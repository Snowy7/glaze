//! Origin metadata retained for settings/plugin permission compatibility.
use std::fmt;
pub const WIDGET_SUFFIX: &str = ".widget.glaze";

/// A web origin, `scheme://host[:port]`. The port is `None` when it's the
/// scheme's default, so `https://a.b` and `https://a.b:443` compare equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin {
    scheme: String,
    host: String,
    port: Option<u16>,
}

impl Origin {
    /// Parses a bare origin, as written in a `network:<origin>` permission:
    /// no path, query, fragment or user info (a trailing `/` is tolerated).
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.strip_suffix('/').unwrap_or(text);
        let (_, after) = rest.split_once("://")?;
        if after.contains(['/', '?', '#', '@']) {
            return None;
        }
        Self::of_url(rest)
    }

    /// The origin of an `http`, `https`, `ws` or `wss` URL.
    pub fn of_url(url: &str) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        let default_port = match scheme.as_str() {
            "http" | "ws" => 80,
            "https" | "wss" => 443,
            _ => return None,
        };
        let authority = rest.split(['/', '?', '#']).next()?;
        let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let (host, port) = match host_port.strip_prefix('[') {
            // [v6]:port
            Some(v6) => {
                let (inside, after) = v6.split_once(']')?;
                let port = match after.strip_prefix(':') {
                    Some(p) => Some(p.parse::<u16>().ok()?),
                    None if after.is_empty() => None,
                    None => return None,
                };
                (format!("[{inside}]"), port)
            }
            None => match host_port.rsplit_once(':') {
                Some((host, p)) => (host.to_owned(), Some(p.parse::<u16>().ok()?)),
                None => (host_port.to_owned(), None),
            },
        };
        if host.is_empty() || host.contains(char::is_whitespace) {
            return None;
        }
        Some(Self {
            scheme,
            host: host.to_ascii_lowercase(),
            port: port.filter(|&p| p != default_port),
        })
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", self.scheme, self.host)?;
        match self.port {
            Some(port) => write!(f, ":{port}"),
            None => Ok(()),
        }
    }
}
