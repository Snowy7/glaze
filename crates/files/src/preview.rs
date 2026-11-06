//! Lazy, bounded preview work. Read at most 64 KiB; shell images are limited to 768 pixels.
//! The worker sleeps on its channel and is released when neither preview surface is open.
use crate::shell;
use mypaper_ui::Image;
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};

pub const TEXT_LIMIT: usize = 64 * 1024;

#[derive(Clone)]
pub enum Content {
    Text { lines: Vec<String>, truncated: bool },
    Image(Arc<Image>),
    Other,
    Error(String),
}

pub struct Service {
    jobs: Sender<(u64, String)>,
    pub replies: Receiver<(u64, Content)>,
    generation: Arc<AtomicU64>,
}

impl Service {
    pub fn new(hwnd: HWND) -> anyhow::Result<Self> {
        let (jobs, requests) = mpsc::channel::<(u64, String)>();
        let (out, replies) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        let hwnd = hwnd.0 as usize;
        std::thread::Builder::new()
            .name("files-preview".into())
            .spawn(move || {
                let Ok(_apartment) = shell::Apartment::new() else {
                    return;
                };
                while let Ok((ticket, path)) = requests.recv() {
                    if ticket != current.load(Ordering::Relaxed) {
                        continue;
                    }
                    let content = load(&path).unwrap_or_else(|e| Content::Error(e.to_string()));
                    if ticket == current.load(Ordering::Relaxed)
                        && out.send((ticket, content)).is_ok()
                    {
                        // SAFETY: only a wake message; no borrowed pointers cross threads.
                        let _ = unsafe {
                            PostMessageW(
                                Some(HWND(hwnd as _)),
                                crate::backend::WAKE,
                                WPARAM(0),
                                LPARAM(0),
                            )
                        };
                    }
                }
            })?;
        Ok(Self {
            jobs,
            replies,
            generation,
        })
    }
    pub fn request(&self, path: String) -> u64 {
        let ticket = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.jobs.send((ticket, path));
        ticket
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}

fn load(path: &str) -> anyhow::Result<Content> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tif" | "tiff" | "ico" | "heic" | "avif"
    ) {
        return Ok(shell::decorate(path, 768, true)?
            .image
            .map_or(Content::Other, Content::Image));
    }
    if text_extension(&extension) {
        let mut bytes = Vec::with_capacity(TEXT_LIMIT + 1);
        std::fs::File::open(path)?
            .take((TEXT_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)?;
        let truncated = bytes.len() > TEXT_LIMIT;
        bytes.truncate(TEXT_LIMIT);
        return Ok(decode(&bytes).map_or(Content::Other, |body| Content::Text {
            lines: body
                .lines()
                .map(|line| line.replace('\t', "    "))
                .collect(),
            truncated,
        }));
    }
    Ok(Content::Other)
}

fn text_extension(ext: &str) -> bool {
    matches!(
        ext,
        "txt"
            | "md"
            | "rs"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "json"
            | "jsonc"
            | "toml"
            | "yaml"
            | "yml"
            | "xml"
            | "html"
            | "css"
            | "scss"
            | "py"
            | "go"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cs"
            | "java"
            | "kt"
            | "swift"
            | "sh"
            | "ps1"
            | "bat"
            | "cmd"
            | "sql"
            | "log"
            | "csv"
            | "ini"
            | "cfg"
            | "conf"
            | "gitignore"
            | "lock"
            | ""
    )
}

fn decode(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        let units: Vec<_> = bytes[2..]
            .as_chunks::<2>().0.iter()
            .map(|p| {
                if little {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    if bytes.contains(&0) {
        return None;
    }
    Some(
        String::from_utf8_lossy(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
            .into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_is_bounded_and_encodings_do_not_execute_content() {
        assert_eq!(decode(b"\xef\xbb\xbfhello").as_deref(), Some("hello"));
        assert_eq!(decode(&[0xff, 0xfe, b'A', 0]).as_deref(), Some("A"));
        assert_eq!(decode(&[0xfe, 0xff, 0, b'A']).as_deref(), Some("A"));
        assert!(decode(b"binary\0data").is_none());
        let root = std::path::Path::new(r"C:\mypaper-targets\files\scratch");
        std::fs::create_dir_all(root).unwrap();
        let path = root.join(format!("preview-limit-{}.txt", std::process::id()));
        std::fs::write(&path, vec![b'x'; TEXT_LIMIT + 500]).unwrap();
        let Content::Text { lines, truncated } = load(path.to_str().unwrap()).unwrap() else {
            panic!("text expected")
        };
        assert!(truncated);
        assert_eq!(lines[0].len(), TEXT_LIMIT);
        std::fs::remove_file(path).unwrap();
    }
}
