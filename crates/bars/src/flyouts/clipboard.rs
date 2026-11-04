//! The clipboard history for the launcher's dashboard layout (docs/bars.md, "Launcher"), on the
//! `clipboard` lane (STA), through `Windows.ApplicationModel.DataTransfer.Clipboard`: Windows' own
//! history (Win+V). It answers an unpackaged process on build 26300 (`examples/winrt-spike.rs`),
//! so there's no history of our own to keep. Picking an entry makes it the clipboard's content;
//! the user pastes it.

use serde::Serialize;
use windows::ApplicationModel::DataTransfer::{
    Clipboard, ClipboardHistoryItem, ClipboardHistoryItemsResultStatus, StandardDataFormats,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

/// The longest text an entry carries.
const PREVIEW: usize = 400;
/// The most entries listed.
const ENTRIES: usize = 25;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Link,
    Image,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub kind: Kind,
    /// The text (cut to a preview), or the formats it has.
    pub text: String,
    /// A small image, as a `data:` URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct History {
    /// Clipboard history is on in Settings.
    pub enabled: bool,
    pub entries: Vec<Entry>,
}

/// The first `PREVIEW` characters, whole.
pub fn preview(text: &str) -> String {
    let trimmed = text.trim();
    match trimmed.char_indices().nth(PREVIEW) {
        Some((at, _)) => format!("{}…", &trimmed[..at]),
        None => trimmed.to_owned(),
    }
}

/// Whether a text entry is a link.
pub fn is_link(text: &str) -> bool {
    let text = text.trim();
    !text.contains(char::is_whitespace)
        && (text.starts_with("https://") || text.starts_with("http://"))
}

pub struct ClipboardLane;

impl ClipboardLane {
    pub fn new() -> Self {
        // SAFETY: COM for this thread (STA), kept for its life.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self
    }

    fn items(&self) -> Option<Vec<ClipboardHistoryItem>> {
        let result = Clipboard::GetHistoryItemsAsync().ok()?.join().ok()?;
        if result.Status().ok()? != ClipboardHistoryItemsResultStatus::Success {
            return None;
        }
        Some(result.Items().ok()?.into_iter().take(ENTRIES).collect())
    }

    pub fn read(&self) -> History {
        let enabled = Clipboard::IsHistoryEnabled().unwrap_or(false);
        let entries = if enabled {
            self.items()
                .unwrap_or_default()
                .iter()
                .filter_map(entry)
                .collect()
        } else {
            Vec::new()
        };
        History { enabled, entries }
    }

    /// Makes entry `index` (as `read` listed it) the clipboard's content.
    pub fn paste(&self, index: usize) -> Result<(), String> {
        let items = self
            .items()
            .ok_or("the clipboard history isn't available")?;
        let item = items.get(index).ok_or("that entry is gone")?;
        Clipboard::SetHistoryItemAsContent(item)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn entry(item: &ClipboardHistoryItem) -> Option<Entry> {
    let content = item.Content().ok()?;
    let has = |format: windows::core::Result<windows::core::HSTRING>| {
        format.and_then(|f| content.Contains(&f)).unwrap_or(false)
    };
    if has(StandardDataFormats::Text()) {
        let text = content.GetTextAsync().ok()?.join().ok()?.to_string();
        return Some(Entry {
            kind: if is_link(&text) {
                Kind::Link
            } else {
                Kind::Text
            },
            text: preview(&text),
            image: None,
        });
    }
    if has(StandardDataFormats::Bitmap()) {
        return Some(Entry {
            kind: Kind::Image,
            text: "Image".into(),
            image: bitmap(&content),
        });
    }
    let formats: Vec<String> = content
        .AvailableFormats()
        .ok()?
        .into_iter()
        .map(|f| f.to_string())
        .collect();
    Some(Entry {
        kind: Kind::Other,
        text: formats.join(", "),
        image: None,
    })
}

/// The bitmap as a `data:` URL, when it's small enough to preview.
fn bitmap(content: &windows::ApplicationModel::DataTransfer::DataPackageView) -> Option<String> {
    let stream = content
        .GetBitmapAsync()
        .ok()?
        .join()
        .ok()?
        .OpenReadAsync()
        .ok()?
        .join()
        .ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > 2 << 20 {
        return None;
    }
    let mime = stream
        .ContentType()
        .map(|t| t.to_string())
        .ok()
        .filter(|t| t.starts_with("image/"))
        .unwrap_or_else(|| "image/png".into());
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size as u32).ok()?.join().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(format!(
        "data:{mime};base64,{}",
        crate::tray_host::base64(&bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_cut_long_text_on_a_character() {
        assert_eq!(preview("  hello \n"), "hello");
        let long = "é".repeat(PREVIEW + 10);
        let cut = preview(&long);
        assert_eq!(cut.chars().count(), PREVIEW + 1);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn links_are_single_urls() {
        assert!(is_link("https://example.com/a?b=c"));
        assert!(!is_link("see https://example.com"));
        assert!(!is_link("example.com"));
    }
}
