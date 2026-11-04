//! Text through DirectWrite: text formats and laid-out strings, cached, so a label is shaped
//! once and redrawn from its `IDWriteTextLayout` while it stays on screen.
//!
//! Families are looked up in the system collection. Icon fonts are just families
//! ([`crate::icons`]); bundled TTFs (Lucide, Fluent UI System Icons, N6) will register into a
//! private collection searched first, with no change to this API.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
    IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::core::{BOOL, HSTRING, w};

/// How a run of text looks. `size` is in device pixels when it reaches the renderer; in
/// [`crate::Tokens`] it is in design pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub family: Arc<str>,
    pub size: f32,
    /// 100..=900, 400 regular, 600 semibold.
    pub weight: u16,
}

impl TextStyle {
    pub fn new(family: &str, size: f32) -> Self {
        Self {
            family: family.into(),
            size,
            weight: 400,
        }
    }

    /// The same style at `scale` times the size.
    pub fn scaled(&self, scale: f32) -> Self {
        Self {
            size: self.size * scale,
            ..self.clone()
        }
    }

    fn key(&self) -> FormatKey {
        FormatKey {
            family: self.family.clone(),
            size: self.size.to_bits(),
            weight: self.weight,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FormatKey {
    family: Arc<str>,
    size: u32,
    weight: u16,
}

/// A laid-out string, its size, and when it was last used.
struct Laid {
    layout: IDWriteTextLayout,
    width: f32,
    height: f32,
    used: u64,
}

/// The text cache of one UI thread.
pub struct Text {
    factory: IDWriteFactory,
    formats: HashMap<FormatKey, IDWriteTextFormat>,
    layouts: HashMap<(FormatKey, Arc<str>), Laid>,
    /// Counts lookups, to find the least recently used layouts.
    clock: u64,
    /// How many layouts to keep ([`MAX_LAYOUTS`] unless the app says otherwise).
    capacity: usize,
}

/// A layout costs about 19 KB private (measured, docs/ui.md), so the cache keeps this many and
/// drops the least recently used half when full; a dropped one rebuilds in microseconds.
const MAX_LAYOUTS: usize = 64;

impl Text {
    pub(crate) fn new() -> Result<Self> {
        // SAFETY: a documented factory call.
        let factory: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.context("DirectWrite")?;
        Ok(Self {
            factory,
            formats: HashMap::new(),
            layouts: HashMap::new(),
            clock: 0,
            capacity: MAX_LAYOUTS,
        })
    }

    pub(crate) fn factory(&self) -> &IDWriteFactory {
        &self.factory
    }

    /// Keeps up to `layouts` laid-out strings (about 19 KB each). A window that shows a few
    /// hundred labels at once (Studio) raises it so a frame never re-shapes its own text.
    pub fn set_capacity(&mut self, layouts: usize) {
        self.capacity = layouts.max(8);
    }

    /// `text` broken into lines no wider than `width` (word wrap; explicit newlines kept).
    /// Each line is then drawn and cached like any label.
    pub fn wrap(&mut self, text: &str, style: &TextStyle, width: f32) -> Vec<String> {
        use windows::Win32::Graphics::DirectWrite::{
            DWRITE_LINE_METRICS, DWRITE_WORD_WRAPPING_WRAP,
        };
        let Some(format) = self.format(style) else {
            return vec![text.to_owned()];
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: documented calls; DirectWrite copies the string; out pointers are locals.
        let metrics = unsafe {
            let Ok(layout) = self
                .factory
                .CreateTextLayout(&wide, &format, width.max(1.0), 1.0e6)
            else {
                return vec![text.to_owned()];
            };
            let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            let mut count = 0u32;
            let _ = layout.GetLineMetrics(None, &mut count);
            let mut metrics = vec![DWRITE_LINE_METRICS::default(); count as usize];
            if layout
                .GetLineMetrics(Some(&mut metrics), &mut count)
                .is_err()
            {
                return vec![text.to_owned()];
            }
            metrics
        };
        let mut at = 0usize;
        metrics
            .iter()
            .map(|m| {
                let end = (at + m.length as usize).min(wide.len());
                let keep = (m.length - m.newlineLength - m.trailingWhitespaceLength) as usize;
                let line = String::from_utf16_lossy(&wide[at..(at + keep).min(end)]);
                at = end;
                line
            })
            .collect()
    }

    /// `text` shortened with an ellipsis to fit `width`, or as is when it fits.
    pub fn fit(&mut self, text: &str, style: &TextStyle, width: f32) -> String {
        use windows::Win32::Graphics::DirectWrite::DWRITE_HIT_TEST_METRICS;
        if self.measure(text, style).0 <= width {
            return text.to_owned();
        }
        let ellipsis = self.measure("…", style).0;
        let Some(format) = self.format(style) else {
            return text.to_owned();
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: documented calls on a temporary layout; out pointers are locals.
        let cut = unsafe {
            let Ok(layout) = self.factory.CreateTextLayout(&wide, &format, 1.0e6, 1.0e6) else {
                return text.to_owned();
            };
            let (mut trailing, mut inside) = (BOOL(0), BOOL(0));
            let mut hit = DWRITE_HIT_TEST_METRICS::default();
            if layout
                .HitTestPoint(
                    (width - ellipsis).max(0.0),
                    0.0,
                    &mut trailing,
                    &mut inside,
                    &mut hit,
                )
                .is_err()
            {
                return text.to_owned();
            }
            hit.textPosition as usize
        };
        let kept = String::from_utf16_lossy(&wide[..cut.min(wide.len())]);
        format!("{}…", kept.trim_end())
    }

    /// The width and height of `text` in `style`, in device pixels.
    pub fn measure(&mut self, text: &str, style: &TextStyle) -> (f32, f32) {
        self.laid(text, style)
            .map_or((0.0, 0.0), |l| (l.width, l.height))
    }

    /// The cached layout of `text` (made on first use).
    pub(crate) fn layout(&mut self, text: &str, style: &TextStyle) -> Option<&IDWriteTextLayout> {
        self.laid(text, style).map(|l| &l.layout)
    }

    /// Is `family` installed?
    pub fn has_family(&self, family: &str) -> bool {
        let mut fonts = None;
        let (mut index, mut exists) = (0u32, BOOL(0));
        // SAFETY: documented calls; the out pointers are valid locals.
        unsafe {
            self.factory
                .GetSystemFontCollection(&mut fonts, false)
                .is_ok()
                && fonts.is_some_and(|fonts| {
                    fonts
                        .FindFamilyName(&HSTRING::from(family), &mut index, &mut exists)
                        .is_ok()
                        && exists.as_bool()
                })
        }
    }

    fn laid(&mut self, text: &str, style: &TextStyle) -> Option<&Laid> {
        self.clock += 1;
        let key = (style.key(), Arc::<str>::from(text));
        match self.layouts.get_mut(&key) {
            Some(laid) => laid.used = self.clock,
            None => {
                if self.layouts.len() >= self.capacity {
                    self.evict();
                }
                let laid = self.make(text, style)?;
                self.layouts.insert(key.clone(), laid);
            }
        }
        self.layouts.get(&key)
    }

    /// Drops the least recently used half of the layouts.
    fn evict(&mut self) {
        let mut ages: Vec<u64> = self.layouts.values().map(|l| l.used).collect();
        ages.sort_unstable();
        let cutoff = ages[ages.len() / 2];
        self.layouts.retain(|_, l| l.used > cutoff);
    }

    fn make(&mut self, text: &str, style: &TextStyle) -> Option<Laid> {
        let format = self.format(style)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: documented calls; DirectWrite copies the string; the out pointer is a local.
        unsafe {
            let layout = self
                .factory
                .CreateTextLayout(&wide, &format, 4096.0, 4096.0)
                .inspect_err(|e| log::debug!("text layout: {e}"))
                .ok()?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).ok()?;
            Some(Laid {
                layout,
                width: metrics.widthIncludingTrailingWhitespace,
                height: metrics.height,
                used: self.clock,
            })
        }
    }

    fn format(&mut self, style: &TextStyle) -> Option<IDWriteTextFormat> {
        let key = style.key();
        if let Some(format) = self.formats.get(&key) {
            return Some(format.clone());
        }
        // SAFETY: documented calls; the strings outlive them.
        let format = unsafe {
            let format = self
                .factory
                .CreateTextFormat(
                    &HSTRING::from(&*style.family),
                    None,
                    DWRITE_FONT_WEIGHT(i32::from(style.weight)),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    style.size,
                    w!("en-us"),
                )
                .inspect_err(|e| log::debug!("text format {}: {e}", style.family))
                .ok()?;
            // Views place text by its measured size; the layout box is only a limit.
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).ok()?;
            format
        };
        self.formats.insert(key, format.clone());
        Some(format)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_cache_stays_bounded_and_keeps_what_is_in_use() {
        let mut text = Text::new().unwrap();
        let style = TextStyle::new("Segoe UI", 13.0);
        let label = text.measure("View", &style);
        assert!(label.0 > 0.0 && label.1 > 0.0);
        for minute in 0..200 {
            text.measure(&format!("09:{minute:03}"), &style);
            text.measure("View", &style); // on screen the whole time
        }
        assert!(text.layouts.len() <= MAX_LAYOUTS);
        let kept = (style.key(), Arc::<str>::from("View"));
        assert!(
            text.layouts.contains_key(&kept),
            "the label in use survives"
        );
        assert_eq!(text.measure("View", &style), label);
    }
}
