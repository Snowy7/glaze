//! Versioned, bounded little-endian framing shared by the host and client.
use std::io::{self, Read, Write};

pub const MAX_FRAME: usize = 8 * 1024 * 1024;
pub const VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Kind {
    Load = 1,
    Call = 2,
    Shutdown = 3,
    Ok = 4,
    Error = 5,
    Event = 6,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub kind: Kind,
    pub request: u64,
    pub plugin: String,
    pub module: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl Frame {
    pub fn write(&self, mut writer: impl Write) -> io::Result<()> {
        let mut body = Vec::new();
        body.extend([VERSION, self.kind as u8]);
        body.extend(self.request.to_le_bytes());
        for field in [&self.plugin, &self.module, &self.name] {
            let len = u16::try_from(field.len()).map_err(|_| invalid("name too long"))?;
            body.extend(len.to_le_bytes());
            body.extend(field.as_bytes());
        }
        if body.len().saturating_add(self.bytes.len()) > MAX_FRAME {
            return Err(invalid("frame too large"));
        }
        body.extend(&self.bytes);
        writer.write_all(&(body.len() as u32).to_le_bytes())?;
        writer.write_all(&body)?;
        writer.flush()
    }

    pub fn read(mut reader: impl Read) -> io::Result<Self> {
        let mut len = [0; 4];
        reader.read_exact(&mut len)?;
        let len = u32::from_le_bytes(len) as usize;
        if !(16..=MAX_FRAME).contains(&len) {
            return Err(invalid("invalid frame length"));
        }
        let mut body = vec![0; len];
        reader.read_exact(&mut body)?;
        if body[0] != VERSION {
            return Err(invalid("unsupported native protocol version"));
        }
        let kind = match body[1] {
            1 => Kind::Load,
            2 => Kind::Call,
            3 => Kind::Shutdown,
            4 => Kind::Ok,
            5 => Kind::Error,
            6 => Kind::Event,
            _ => return Err(invalid("unknown frame kind")),
        };
        let request = u64::from_le_bytes(body[2..10].try_into().unwrap());
        let mut offset = 10;
        let mut field = || -> io::Result<String> {
            let bytes = body
                .get(offset..offset + 2)
                .ok_or_else(|| invalid("truncated name"))?;
            let len = u16::from_le_bytes(bytes.try_into().unwrap()) as usize;
            offset += 2;
            let text = body
                .get(offset..offset + len)
                .ok_or_else(|| invalid("truncated name"))?;
            offset += len;
            String::from_utf8(text.to_vec()).map_err(|_| invalid("invalid UTF-8 name"))
        };
        let plugin = field()?;
        let module = field()?;
        let name = field()?;
        Ok(Self {
            kind,
            request,
            plugin,
            module,
            name,
            bytes: body[offset..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_reject_malformed() {
        let frame = Frame {
            kind: Kind::Call,
            request: 42,
            plugin: "echo".into(),
            module: "echo.dll".into(),
            name: "echo".into(),
            bytes: vec![0, 255, 1],
        };
        let mut buffer = Vec::new();
        frame.write(&mut buffer).unwrap();
        assert_eq!(Frame::read(&buffer[..]).unwrap(), frame);
        assert!(Frame::read(&buffer[..buffer.len() - 1]).is_err());
        buffer[4] = 255;
        assert!(Frame::read(&buffer[..]).is_err());
        assert!(Frame::read(&u32::MAX.to_le_bytes()[..]).is_err());
    }
}
