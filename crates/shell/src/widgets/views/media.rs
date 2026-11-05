//! Now playing and the media pill. The box picks the layout, as the web widget's container
//! queries did:
//! - **compact** (under 165 px tall, the strip): the cover the full height, title and artist,
//!   a hairline of progress, the transport at the right;
//! - **row** (3:2 and wider): the cover on the left; the app, title, artist, scrubber with
//!   times and the transport in a column beside it (the macOS Now Playing widget);
//! - **column** (square, portrait): the cover on top, the same column centred under it;
//! - **pill**: a capsule with a small cover, two lines and glyph controls.
//!
//! The cover's colour (lifted to read on glass) tints the play disc and the progress.
use super::{Ctx, NEXT, PREVIOUS, PRIMARY};
use crate::widgets::media_time;
use mypaper_ui::icons::Icon;
use mypaper_ui::{Align, Op, Rect, Rgba, Role, TextRole, TextStyle};

/// What every layout shows.
struct Track<'a> {
    title: &'a str,
    artist: &'a str,
    source: String,
    playing: bool,
    /// Position and duration, seconds.
    timeline: Option<(f64, f64)>,
    /// The play disc's fill and glyph.
    disc: (Rgba, Rgba),
    /// The progress fill.
    tint: Rgba,
}

fn track<'a>(c: &Ctx<'a>) -> Track<'a> {
    let m = c.m;
    let color = c.color();
    let playing = m.data["playing"].as_bool() == Some(true);
    let advance = if playing {
        (c.now.ms - m.data["_at"].as_i64().unwrap_or(c.now.ms)).max(0) as f64 / 1000.0
    } else {
        0.0
    };
    let timeline = m
        .flag("progress", true)
        .then(|| {
            let duration = m.data["duration"].as_f64().filter(|d| *d > 0.0)?;
            let position = (m.data["position"].as_f64()? + advance).min(duration);
            Some((position, duration))
        })
        .flatten();
    let tint = m
        .artwork_tint
        .filter(|_| m.flag("tint", true))
        .map(|t| lift(t, if c.f.theme.dark() { 0.68 } else { 0.40 }));
    let disc = tint.map_or((color.accent, color.on_accent), |t| {
        let on = if mypaper_ui::theme::luminance(t) > 0.4 {
            Rgba::rgb8([17, 17, 19], 1.0)
        } else {
            Rgba::rgb8([255; 3], 1.0)
        };
        (t, on)
    });
    Track {
        title: m.data["title"].as_str().unwrap_or(""),
        artist: m.data["artist"].as_str().unwrap_or(""),
        source: app_name(m.data["app"].as_str().unwrap_or("")).to_uppercase(),
        playing,
        timeline,
        disc,
        tint: tint.unwrap_or(color.label),
    }
}

pub fn draw(c: &mut Ctx, r: Rect) {
    let t = track(c);
    if c.m.placed.widget == "media-pill" {
        return pill(c, &t);
    }
    let color = c.color();
    let s = c.s();
    if c.m.str("variant", "card") == "blur"
        && !t.title.is_empty()
        && let Some(image) = &c.m.artwork_backdrop
    {
        let radius = super::plate_radius(c.m, c.rect, s);
        let rect = c.rect;
        cover_image(c, rect, image, 0.55);
        c.f.fill(rect, radius, color.shadow.with_alpha(0.35));
    }
    if t.title.is_empty() {
        return idle(c, r);
    }
    let (w, h) = (c.rect.width() / s, c.rect.height() / s);
    // The row and column layouts need about 165 px for cover, text and transport.
    if c.m.str("variant", "card") == "strip" || h < 165.0 {
        compact(c, r, &t);
    } else if w >= h * 1.5 {
        row(c, r, &t);
    } else {
        column(c, r, &t);
    }
    if c.m.str("variant", "card") == "viz" && !c.m.spectrum.is_empty() {
        spectrum(c, r, t.tint);
    }
}

/// Nothing playing: a note, the line, and the transport dimmed.
fn idle(c: &mut Ctx, r: Rect) {
    let color = c.color();
    if r.height() >= c.px(110.0) {
        c.empty(r, Some(Icon::Music), "Nothing playing", Some("Start music in any app"));
        return;
    }
    let side = r.height().min(c.px(56.0));
    let (art, rest) = r.take_left(side, c.px(12.0));
    let art = art.place(side, side, Align::Start, Align::Center);
    c.f.fill(art, (side * 0.12).round(), color.card);
    c.icon(art, Icon::Music, (side / c.s() * 0.4).round(), color.tertiary);
    let (a, b) = (c.f.line(TextRole::Headline), c.f.line(TextRole::Footnote));
    let y = rest.top + ((rest.height() - a - b) / 2.0).round();
    c.text(Rect::new(rest.left, y, rest.width(), a), "Nothing playing", TextRole::Headline, color.secondary, Align::Start);
    c.text(Rect::new(rest.left, y + a, rest.width(), b), "Start music in any app", TextRole::Footnote, color.tertiary, Align::Start);
}

/// The cover, or a quiet square with a note.
fn cover(c: &mut Ctx, rect: Rect) {
    let color = c.color();
    match c.m.artwork.clone() {
        Some(image) => cover_image(c, rect, &image, 1.0),
        None => {
            c.f.fill(rect, (rect.width() * 0.12).round(), color.card);
            c.icon(rect, Icon::Music, (rect.width() / c.s() * 0.36).clamp(14.0, 56.0), color.tertiary);
        }
    }
}

/// `image` covering `rect` (a centred crop, never stretched).
fn cover_image(c: &mut Ctx, rect: Rect, image: &std::sync::Arc<mypaper_ui::Image>, opacity: f32) {
    let ratio = (rect.width() / image.width as f32).max(rect.height() / image.height as f32);
    let (w, h) = (rect.width() / ratio, rect.height() / ratio);
    c.f.push(Op::Image {
        rect,
        image: image.clone(),
        opacity,
        source: Some(Rect::new(
            (image.width as f32 - w) / 2.0,
            (image.height as f32 - h) / 2.0,
            w,
            h,
        )),
    });
}

/// Previous, play/pause on its disc, next, centred on `cy` from `x` (or centred in `span`).
fn transport(c: &mut Ctx, span: Rect, play: f32, skip: f32, t: &Track, align: Align, skips: bool) {
    let gap = c.px(6.0);
    let width = if skips { play + 2.0 * (skip + gap) } else { play };
    let row = span.place(width, play, align, Align::Center);
    let mut x = row.left;
    let s = c.s();
    if skips {
        let at = Rect::new(x, row.top, skip, play);
        c.f.icon_button(PREVIOUS, at, Icon::Previous, skip / s, "Previous track", false);
        x += skip + gap;
    }
    let disc = Rect::new(x, row.top, play, play);
    let id = PRIMARY;
    c.f.fill(disc, play / 2.0, t.disc.0);
    c.f.wash(id, disc, play / 2.0, true);
    c.f.focus_ring(id, disc, play / 2.0);
    let glyph = (play / s * 0.42).round();
    c.icon(disc, if t.playing { Icon::Pause } else { Icon::Play }, glyph, t.disc.1);
    c.f.hit(id, disc, Role::Button, "Play or pause", true, None);
    x += play + gap;
    if skips {
        let at = Rect::new(x, row.top, skip, play);
        c.f.icon_button(NEXT, at, Icon::Next, skip / s, "Next track", false);
    }
}

/// The scrubber: elapsed, the track, time left (times only when there is room).
fn scrubber(c: &mut Ctx, r: Rect, t: &Track, times: bool) {
    let Some((position, duration)) = t.timeline else { return };
    let color = c.color();
    let style = c.f.font(TextRole::Caption);
    let elapsed = media_time(position);
    let left = format!("-{}", media_time(duration - position));
    let gutter = if times {
        c.f.text.measure("-00:00", &style).0 + c.px(6.0)
    } else {
        0.0
    };
    let bar_h = c.px(4.0);
    let track = Rect::new(
        r.left + gutter,
        (r.top + (r.height() - bar_h) / 2.0).round(),
        r.width() - 2.0 * gutter,
        bar_h,
    );
    c.f.progress(track, (position / duration) as f32, Some(t.tint));
    if times {
        c.styled(Rect { right: r.left + gutter, ..r }, &elapsed, &style, color.secondary, Align::Start, Align::Center);
        c.styled(Rect { left: r.right - gutter, ..r }, &left, &style, color.secondary, Align::End, Align::Center);
    }
}

/// A title style that fits `room` in `lines` lines at the largest size from `max` down to `min`.
fn title_style(c: &mut Ctx, title: &str, min: f32, max: f32, room: f32, lines: usize) -> TextStyle {
    let mut size = max;
    loop {
        let style = c.display(size, 600);
        if size <= min || c.f.text.lines(title, &style, room, lines + 1).len() <= lines {
            return style;
        }
        size -= 1.0;
    }
}

/// Up to `lines` wrapped lines of `text` from `y`; the height used.
fn lines(c: &mut Ctx, rect: Rect, text: &str, style: &TextStyle, color: Rgba, lines: usize, align: Align) -> f32 {
    let wrapped = c.f.text.lines(text, style, rect.width(), lines);
    let line = (style.size * 1.22).round();
    for (i, text) in wrapped.iter().enumerate() {
        c.styled(Rect::new(rect.left, rect.top + i as f32 * line, rect.width(), line), text, style, color, align, Align::Center);
    }
    line * wrapped.len() as f32
}

/// The app's name with three small level bars while playing.
fn eyebrow(c: &mut Ctx, r: Rect, t: &Track, align: Align) {
    if t.source.is_empty() {
        return;
    }
    let color = c.color();
    let style = mypaper_ui::TextStyle {
        tracking: 0.1 * c.px(10.0),
        ..c.display(10.5, 650)
    };
    let w = c.f.text.measure(&t.source, &style).0;
    let bars = if t.playing { c.px(14.0) } else { 0.0 };
    let group = r.place(w + bars, r.height(), align, Align::Center);
    if t.playing {
        let unit = c.px(3.0);
        for (i, level) in [0.55, 1.0, 0.75].into_iter().enumerate() {
            let bh = (r.height() * 0.55 * level).round().max(unit);
            let x = group.left + i as f32 * (unit + c.px(1.0));
            c.f.fill(Rect::new(x, (group.center().y + r.height() * 0.275 - bh).round(), unit - 1.0, bh), 1.0, t.tint.with_alpha(0.85));
        }
    }
    c.styled(Rect { left: group.left + bars, ..group }, &t.source, &style, color.tertiary, Align::Start, Align::Center);
}

/// Cover left, the column beside it.
fn row(c: &mut Ctx, r: Rect, t: &Track) {
    let color = c.color();
    let s = c.s();
    let side = r.height().min(r.width() * 0.44).floor();
    let (art, rest) = r.take_left(side, (r.width() * 0.05).clamp(c.px(14.0), c.px(24.0)));
    cover(c, art.place(side, side, Align::Start, Align::Center));
    let h = c.rect.height() / s;
    let max = (h * 0.115).clamp(18.0, 30.0);
    let title = title_style(c, t.title, 15.0, max, rest.width(), 2);
    let artist = c.display((h * 0.07).clamp(13.0, 16.0), 450);
    let play = (h * 0.2).clamp(38.0, 50.0).round() * s;
    let skip = (play * 0.78).round();
    let brow = if h >= 150.0 { c.px(16.0) } else { 0.0 };
    let title_lines = c.f.text.lines(t.title, &title, rest.width(), 2).len().max(1) as f32;
    let scrub = if t.timeline.is_some() { c.px(22.0) } else { 0.0 };
    let block = brow
        + title_lines * (title.size * 1.22).round()
        + (artist.size * 1.4).round()
        + scrub
        + c.px(8.0)
        + play;
    let mut y = rest.top + ((rest.height() - block) / 2.0).max(0.0).round();
    if brow > 0.0 {
        eyebrow(c, Rect::new(rest.left, y, rest.width(), brow), t, Align::Start);
        y += brow + c.px(2.0);
    }
    y += lines(c, Rect::new(rest.left, y, rest.width(), 0.0), t.title, &title, color.label, 2, Align::Start);
    let ah = (artist.size * 1.4).round();
    c.styled(Rect::new(rest.left, y, rest.width(), ah), t.artist, &artist, color.secondary, Align::Start, Align::Center);
    y += ah;
    if scrub > 0.0 {
        scrubber(c, Rect::new(rest.left, y + c.px(4.0), rest.width(), scrub - c.px(4.0)), t, rest.width() > c.px(180.0));
        y += scrub;
    }
    y += c.px(8.0);
    // The previous glyph's ink lines up with the text above (its round button starts a
    // little to the left of it).
    let skips = rest.width() > c.px(150.0);
    let nudge = if skips { (skip - c.px(18.0)) / 2.0 } else { 0.0 };
    let span = Rect::new((rest.left - nudge).round(), y, rest.width(), play);
    transport(c, span, play, skip, t, Align::Start, skips);
}

/// Cover on top, the column centred under it.
fn column(c: &mut Ctx, r: Rect, t: &Track) {
    let color = c.color();
    let s = c.s();
    let h = c.rect.height() / s;
    let tall = h >= 300.0;
    let small = h <= 215.0;
    let min_side = c.rect.width().min(c.rect.height()) / s;
    let title_max = if tall { (min_side * 0.07).clamp(20.0, 28.0) } else if small { 16.0 } else { 18.0 };
    let title = title_style(c, t.title, 14.0, title_max, r.width(), 1);
    let artist = c.display(if tall { (min_side * 0.044).clamp(14.0, 17.0) } else if small { 12.0 } else { 14.0 }, 450);
    let play = c.px(if small { 38.0 } else { 44.0 });
    let skip = c.px(if small { 30.0 } else { 34.0 });
    let scrub = if t.timeline.is_some() { c.px(if small { 12.0 } else { 22.0 }) } else { 0.0 };
    let text_h = (title.size * 1.22).round() + (artist.size * 1.4).round();
    let below = text_h + scrub + c.px(6.0) + play;
    let gap = c.px(if small { 8.0 } else { 12.0 });
    let side = (r.height() - below - gap).min(r.width()).clamp(c.px(48.0), c.px(360.0)).floor();
    let total = side + gap + below;
    let mut y = r.top + ((r.height() - total) / 2.0).max(0.0).round();
    cover(c, Rect::new(r.left + ((r.width() - side) / 2.0).round(), y, side, side));
    y += side + gap;
    let th = (title.size * 1.22).round();
    c.styled(Rect::new(r.left, y, r.width(), th), t.title, &title, color.label, Align::Center, Align::Center);
    y += th;
    let ah = (artist.size * 1.4).round();
    c.styled(Rect::new(r.left, y, r.width(), ah), t.artist, &artist, color.secondary, Align::Center, Align::Center);
    y += ah;
    if scrub > 0.0 {
        scrubber(c, Rect::new(r.left, y, r.width(), scrub), t, !small && r.width() > c.px(180.0));
        y += scrub;
    }
    y += c.px(6.0);
    transport(c, Rect::new(r.left, y, r.width(), play), play, skip, t, Align::Center, r.width() > c.px(120.0));
}

/// A short strip: cover, two lines over a hairline of progress, the transport at the right.
fn compact(c: &mut Ctx, r: Rect, t: &Track) {
    let color = c.color();
    let s = c.s();
    let h = c.rect.height() / s;
    let w = c.rect.width() / s;
    let side = r.height().floor();
    let play = (h * 0.52).clamp(32.0, 40.0).round() * s;
    if w <= 249.0 {
        // The cover with play on it, the lines beside.
        let (art, rest) = r.take_left(side, c.px(12.0));
        cover(c, art);
        let d = c.px(30.0);
        let disc = art.place(d, d, Align::Center, Align::Center);
        c.f.fill(disc, d / 2.0, color.shadow.with_alpha(0.5));
        c.f.wash(PRIMARY, disc, d / 2.0, true);
        c.f.focus_ring(PRIMARY, disc, d / 2.0);
        c.icon(disc, if t.playing { Icon::Pause } else { Icon::Play }, 13.0, Rgba::rgb8([255; 3], 1.0));
        c.f.hit(PRIMARY, disc, Role::Button, "Play or pause", true, None);
        two_lines(c, rest, t, h);
        return;
    }
    let skips = w > 339.0;
    let skip = (play * 0.8).round();
    let controls = if skips { play + 2.0 * (skip + c.px(6.0)) } else { play };
    let (art, rest) = r.take_left(side, c.px(12.0));
    cover(c, art);
    let (buttons, text) = rest.take_right(controls, c.px(10.0));
    transport(c, buttons, play, skip, t, Align::End, skips);
    two_lines(c, text, t, h);
}

/// Title over artist, centred in `r`, with a hairline of progress under them.
fn two_lines(c: &mut Ctx, r: Rect, t: &Track, h: f32) {
    let color = c.color();
    let title = c.display((h * 0.24).clamp(14.0, 18.0), 600);
    let artist = c.f.font(TextRole::Callout);
    let (a, b) = ((title.size * 1.25).round(), c.f.line(TextRole::Callout));
    let bar = if t.timeline.is_some() { c.px(9.0) } else { 0.0 };
    let y = r.top + ((r.height() - a - b - bar) / 2.0).round();
    c.styled(Rect::new(r.left, y, r.width(), a), t.title, &title, color.label, Align::Start, Align::Center);
    c.styled(Rect::new(r.left, y + a, r.width(), b), t.artist, &artist, color.secondary, Align::Start, Align::Center);
    if let Some((position, duration)) = t.timeline {
        let track = Rect::new(r.left, y + a + b + c.px(6.0), r.width(), c.px(3.0));
        c.f.progress(track, (position / duration) as f32, Some(t.tint));
    }
}

/// The pill: a capsule (cover, title and artist, glyph controls); `mini` keeps cover and play;
/// `notch` is always the dark island.
fn pill(c: &mut Ctx, t: &Track) {
    let color = c.color();
    let s = c.s();
    let variant = c.m.str("variant", "pill");
    let rect = c.rect;
    if variant == "notch" {
        c.f.fill(rect, super::plate_radius(c.m, rect, s), color.surface.with_alpha(0.96));
    }
    let h = rect.height();
    let pad = ((h - (h - c.px(16.0)).clamp(c.px(44.0), c.px(72.0))) / 2.0).max(c.px(6.0)) + c.px(4.0);
    let inner = rect.inset(mypaper_ui::Insets::xy((h / 2.0 - c.px(10.0)).max(pad), pad));
    let cap_h = inner.height();
    let art_side = (cap_h).floor();
    if t.title.is_empty() {
        let (art, rest) = inner.take_left(art_side, c.px(10.0));
        c.f.fill(art, art_side * 0.3, color.card);
        c.icon(art, Icon::Music, (art_side / s * 0.42).round(), color.tertiary);
        c.text(rest, "Nothing playing", TextRole::Headline, color.secondary, Align::Start);
        return;
    }
    let (art, rest) = inner.take_left(art_side, c.px(10.0));
    cover(c, art);
    let play = (rect.height() * 0.52).min(cap_h + c.px(6.0)).min(c.px(40.0)).round();
    let skip = (rect.height() * 0.44).min(cap_h).min(c.px(32.0)).round();
    let mini = variant == "mini";
    let skips = !mini && rect.width() / s > 260.0;
    let gap = c.px(2.0);
    let controls = if skips { play + 2.0 * (skip + gap) } else { play };
    let (buttons, text) = rest.take_right(controls, c.px(8.0));
    let row = buttons.place(controls, play, Align::End, Align::Center);
    let mut x = row.left;
    if skips {
        c.f.icon_button(PREVIOUS, Rect::new(x, row.top, skip, play), Icon::Previous, skip / s, "Previous track", false);
        x += skip + gap;
    }
    c.f.icon_button(PRIMARY, Rect::new(x, row.top, play, play), if t.playing { Icon::Pause } else { Icon::Play }, play / s, "Play or pause", false);
    x += play + gap;
    if skips {
        c.f.icon_button(NEXT, Rect::new(x, row.top, skip, play), Icon::Next, skip / s, "Next track", false);
    }
    if mini {
        return;
    }
    let hh = rect.height() / s;
    let title = c.display((hh * 0.19).clamp(12.0, 14.0), 600);
    let artist = c.display((hh * 0.16).clamp(11.0, 12.5), 400);
    let (a, b) = ((title.size * 1.3).round(), (artist.size * 1.35).round());
    let bar = if t.timeline.is_some() { c.px(7.0) } else { 0.0 };
    let y = text.top + ((text.height() - a - b - bar) / 2.0).round();
    c.styled(Rect::new(text.left, y, text.width(), a), t.title, &title, color.label, Align::Start, Align::Center);
    c.styled(Rect::new(text.left, y + a, text.width(), b), t.artist, &artist, color.secondary, Align::Start, Align::Center);
    if let Some((position, duration)) = t.timeline {
        let track = Rect::new(text.left, y + a + b + c.px(4.0), text.width(), c.px(3.0));
        c.f.progress(track, (position / duration) as f32, Some(t.tint));
    }
}

/// The visualizer: rounded bars along the bottom, in the cover's colour.
fn spectrum(c: &mut Ctx, r: Rect, tint: Rgba) {
    let band = Rect { top: r.bottom - c.px(26.0), ..r };
    let n = c.m.spectrum.len();
    let w = band.width() / n as f32;
    let gain = c.m.num("sensitivity", 1.2) as f32;
    let bar = (w - c.px(2.0)).max(c.px(2.0));
    for (i, level) in c.m.spectrum.iter().enumerate() {
        let h = ((level * gain).clamp(0.06, 1.0) * band.height()).round();
        c.f.fill(
            Rect::new((band.left + i as f32 * w).round(), band.bottom - h, bar, h),
            bar / 2.0,
            tint.with_alpha(0.45),
        );
    }
}

/// A cover's average colour as a highlight at `lightness` (the web widget's
/// `hsl(var(--art-h) var(--art-s) 68%)`; 40% under dark text), so it reads on glass.
fn lift(c: Rgba, lightness: f32) -> Rgba {
    let (max, min) = (c.r.max(c.g).max(c.b), c.r.min(c.g).min(c.b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let sat = if d < 1e-4 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    let hue = if d < 1e-4 {
        0.0
    } else if max == c.r {
        ((c.g - c.b) / d).rem_euclid(6.0)
    } else if max == c.g {
        (c.b - c.r) / d + 2.0
    } else {
        (c.r - c.g) / d + 4.0
    };
    let sat = sat.clamp(0.0, 0.8);
    let chroma = (1.0 - (2.0 * lightness - 1.0f32).abs()) * sat;
    let x = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    Rgba::new(r + m, g + m, b + m, 1.0)
}

/// A media source's display name from what Windows reports: a packaged app's id
/// ("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify") gives "Spotify", an executable
/// ("chrome.exe") gives "Chrome".
fn app_name(app: &str) -> String {
    let app = app.split('!').next().unwrap_or(app);
    if let Some((family, _)) = app.split_once('_') {
        // The package name's last part, up to its second capitalised word.
        let last = family.rsplit('.').next().unwrap_or(family);
        let end = last
            .char_indices()
            .skip(1)
            .find(|(_, c)| c.is_uppercase())
            .map_or(last.len(), |(i, _)| i);
        last[..end].to_owned()
    } else {
        let stem = app.rsplit(['\\', '/']).next().unwrap_or(app);
        let stem = stem.strip_suffix(".exe").unwrap_or(stem);
        let mut chars = stem.chars();
        chars
            .next()
            .map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
    }
}

#[test]
fn media_sources_read_as_app_names() {
    assert_eq!(app_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "Spotify");
    assert_eq!(app_name("chrome.exe"), "Chrome");
    assert_eq!(app_name(r"C:\Apps\foobar2000.exe"), "Foobar2000");
}
