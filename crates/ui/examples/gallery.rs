//! The kit's gallery: every component in all five styles and both schemes, over a wallpaper,
//! written as PNGs so they can be judged side by side (docs/ui.md, "The kit").
//!
//!   cargo run -p mypaper-ui --example gallery [-- <out dir>]   (default docs/media/native)
//!
//! Writes `kit-<style>-<scheme>.png`, `kit-text.png` (the text lab) and `kit-icons.png` (vector
//! icons against the icon font). Glass is a stand-in for the engine's: the wallpaper blurred,
//! the material's tint, and one rim, so the content can be judged over it. The panels are
//! drawn at 1x (96 DPI) and the PNG is scaled up 2x without smoothing, to show real pixels.

use std::path::{Path, PathBuf};
use std::time::Instant;

use mypaper_ui::icons::Icon;
use mypaper_ui::interact::Interaction;
use mypaper_ui::kit::{Accessory, ButtonKind, Frame, Lead, Row, Tile};
use mypaper_ui::layout::{Align, Insets, Stack, grid};
use mypaper_ui::{
    DisplayList, Entry, Image, Point, Rect, Rgba, Scheme, Scroll, Style, TextMode, TextQuality,
    TextRole, TextStyle, Theme, Ui, render,
};

const W: u32 = 1180;
const H: u32 = 780;

fn main() -> anyhow::Result<()> {
    let out: PathBuf = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("docs/media/native"), PathBuf::from);
    std::fs::create_dir_all(&out)?;
    let mut ui = Ui::new()?;
    let wallpaper = std::fs::read("themes/rain-city/preview.jpg")?;
    let backdrop = cover(&ui.decode_image(&wallpaper, (W * 2, H * 2))?, W, H);
    let blurred = blur(&backdrop, 18);
    for style in Style::ALL {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let theme = Theme::new(style, scheme, mypaper_ui::theme::SYSTEM_BLUE);
            let started = Instant::now();
            let (list, glass) = gallery(&mut ui, &theme);
            let built = started.elapsed();
            let image = render(&mut ui, &list, W, H)?;
            let drawn = started.elapsed();
            let mut bg = backdrop.clone();
            for (rect, radius) in &glass {
                glass_stand_in(&mut bg, &blurred, *rect, *radius, &theme);
            }
            let name = format!("kit-{}-{}.png", style.name(), scheme_name(scheme));
            save(&out.join(&name), &over(&image, &bg), 2)?;
            println!(
                "{name}: built in {:.2} ms, drawn in {:.2} ms",
                built.as_secs_f64() * 1e3,
                (drawn - built).as_secs_f64() * 1e3
            );
        }
    }
    text_lab(&mut ui, &out, &backdrop, &blurred)?;
    icon_sheet(&mut ui, &out, &backdrop, &blurred)?;
    Ok(())
}

fn scheme_name(s: Scheme) -> &'static str {
    match s {
        Scheme::Dark => "dark",
        Scheme::Light => "light",
    }
}

/// Three panels: Control Center, a scrolled notification list, and controls with their
/// states; a menu and a tooltip float beside them. Returns the list and the glass shapes.
fn gallery(ui: &mut Ui, t: &Theme) -> (DisplayList, Vec<(Rect, f32)>) {
    // Hover a row, press a button, keyboard-focus a switch.
    let input = Interaction {
        hover: Some(31),
        pressed: Some(42),
        focus: Some(50),
        focus_visible: true,
    };
    let mut f = Frame::new(&mut ui.text, t, 1.0, &input);
    let mut glass = Vec::new();
    let panel = |x: f32, h: f32| Rect::new(x, 24.0, 360.0, h);

    // 1. Control Center.
    let a = panel(24.0, 560.0);
    if let Some(g) = f.surface(a, true) {
        glass.push((g.body, g.radius));
    }
    let pad = f.px(t.metrics.pad);
    let content = a.inset(Insets::all(pad));
    let (head, rest) = content.take_top(f.px(32.0), f.px(10.0));
    f.text_in(head, "Control Center", TextRole::Title2, t.color.label, Align::Start);
    let (power, rest_head) = head.take_right(f.px(30.0), f.px(4.0));
    f.icon_button(10, power, Icon::Power, 30.0, "Power", false);
    let (settings, _) = rest_head.take_right(f.px(30.0), 0.0);
    f.icon_button(11, settings, Icon::Settings, 30.0, "Settings", false);
    let mut col = Stack::column(rest, f.px(t.metrics.gap));
    let tiles_rect = col.next(f.px(56.0) * 2.0 + f.px(8.0));
    let tiles = [
        (Icon::Wifi(3), "Wi-Fi", "Home 5G", true),
        (Icon::Bluetooth, "Bluetooth", "AirPods", true),
        (Icon::Moon, "Focus", "Off", false),
        (Icon::NightLight, "Night light", "Until 7:00", false),
    ];
    for (i, (icon, title, sub, on)) in tiles.into_iter().enumerate() {
        let r = grid(tiles_rect, 2, f.px(8.0), f.px(56.0), i);
        f.tile(
            &Tile {
                id: 20 + i as u32,
                icon,
                title,
                subtitle: sub,
                on,
                enabled: true,
                more: (i < 2).then_some(24 + i as u32),
            },
            r,
        );
    }
    let sliders = col.next(f.px(108.0));
    let inner = f.card(sliders);
    let (label, rest) = inner.take_top(f.line(TextRole::Subheadline), f.px(4.0));
    f.text_in(label, "Display", TextRole::Subheadline, t.color.secondary, Align::Start);
    let (bar, rest) = rest.take_top(f.px(28.0), f.px(6.0));
    f.slider(30, bar, 0.7, "Brightness", Some(Icon::Brightness));
    let (_, rest) = rest.take_top(0.0, 0.0);
    f.slider(32, rest.take_top(f.px(28.0), 0.0).0, 0.4, "Volume", Some(Icon::Volume(2)));
    // Media.
    let media = col.next(f.px(76.0));
    let inner = f.card(media);
    let (art, text) = inner.take_left(f.px(52.0), f.px(12.0));
    let art = art.place(f.px(52.0), f.px(52.0), Align::Start, Align::Center);
    f.fill(art, f.px(t.radius.small + 2.0), t.color.accent.with_alpha(0.55));
    f.icon(art, Icon::Music, 22.0, t.color.on_accent);
    let (controls, text) = text.take_right(f.px(3.0 * 34.0 + 8.0), f.px(8.0));
    let block = text.place(text.width(), f.line(TextRole::Headline) + f.line(TextRole::Callout) + f.px(10.0), Align::Start, Align::Center);
    let (title, text) = block.take_top(f.line(TextRole::Headline), 0.0);
    f.text_in(title, "Kiptime", TextRole::Headline, t.color.label, Align::Start);
    let (artist, bar) = text.take_top(f.line(TextRole::Callout), f.px(6.0));
    f.text_in(artist, "brillion.", TextRole::Callout, t.color.secondary, Align::Start);
    f.progress(bar.take_top(f.px(4.0), 0.0).0, 0.35, None);
    let mut row = Stack::row(controls, f.px(4.0));
    f.icon_button(40, row.next(f.px(34.0)), Icon::Previous, 34.0, "Previous", false);
    f.icon_button(41, row.next(f.px(34.0)), Icon::Pause, 34.0, "Pause", true);
    f.icon_button(42, row.next(f.px(34.0)), Icon::Next, 34.0, "Next", false);
    // Toggles in a card.
    let toggles = col.next(f.px(2.0 * t.metrics.row_single + 12.0));
    f.card(toggles);
    let inner = toggles.inset(Insets::all(f.px(6.0)));
    let mut rows = Stack::column(inner, 0.0);
    let r1 = rows.next(f.px(t.metrics.row_single));
    f.row(
        &Row {
            id: 50,
            lead: Lead::Badge(Icon::Airplane, false),
            title: "Airplane mode",
            subtitle: None,
            accessory: Accessory::Switch(false),
            selected: false,
            enabled: true,
        },
        r1,
    );
    let r2 = rows.next(f.px(t.metrics.row_single));
    f.row(
        &Row {
            id: 51,
            lead: Lead::Badge(Icon::Hotspot, true),
            title: "Mobile hotspot",
            subtitle: None,
            accessory: Accessory::Switch(true),
            selected: false,
            enabled: true,
        },
        r2,
    );

    // 2. Notifications, scrolled, with its scrollbar showing.
    let b = panel(408.0, 560.0);
    if let Some(g) = f.surface(b, true) {
        glass.push((g.body, g.radius));
    }
    let content = b.inset(Insets::all(pad));
    let (head, rest) = content.take_top(f.px(32.0), f.px(6.0));
    f.text_in(head, "Notifications", TextRole::Title2, t.color.label, Align::Start);
    let (clear, rest_head) = head.take_right(f.px(30.0), f.px(4.0));
    f.icon_button(12, clear, Icon::Close, 30.0, "Clear all", false);
    let (bell, _) = rest_head.take_right(f.px(30.0), 0.0);
    f.icon_button(13, bell, Icon::BellOff, 30.0, "Do not disturb", false);
    let (viewport, footer) = rest.take_bottom(f.px(120.0), f.px(10.0));
    let (footer, viewport) = (viewport, footer);
    let mut scroll = Scroll::new(&t.motion);
    let now = Instant::now();
    // Lay out once to learn the extent, then scroll a little (as after a touchpad flick).
    let items = [
        ("Messages", "Lena", "Are we still on for tonight? I booked the table at eight.", "now"),
        ("Calendar", "Design review", "Starts in 15 minutes · Room 4", "5 min"),
        ("Mail", "Your order has shipped", "It will arrive on Thursday between 10 and 2.", "1 h"),
        ("Photos", "Memories", "Rain City, three years ago today.", "3 h"),
        ("Settings", "Updates are ready", "Restart to finish installing.", "Yesterday"),
    ];
    let draw_list = |f: &mut Frame, at: Point| -> f32 {
        let mut stack = Stack::column(Rect::new(at.x, at.y, viewport.width(), 10_000.0), f.px(8.0));
        for (i, (app, title, body, when)) in items.iter().enumerate() {
            let lines = f.line_count(body, TextRole::Callout, viewport.width() - 2.0 * pad, 2);
            let h = pad * 2.0
                + f.line(TextRole::Footnote)
                + f.px(2.0)
                + f.line(TextRole::Headline)
                + lines as f32 * f.line(TextRole::Callout);
            let card = stack.next(h);
            f.card(card);
            let id = 60 + i as u32;
            let radius = f.px(t.radius.card);
            f.wash(id, card, radius, true);
            let inner = card.inset(Insets::all(pad));
            let (top, rest) = inner.take_top(f.line(TextRole::Footnote), f.px(2.0));
            let (icon, top_text) = top.take_left(f.px(16.0), f.px(6.0));
            f.icon(icon, Icon::Bell, 13.0, t.color.secondary);
            let (time, app_rect) = top_text.take_right(f.px(70.0), f.px(6.0));
            f.text_in(app_rect, app, TextRole::Footnote, t.color.secondary, Align::Start);
            f.text_in(time, when, TextRole::Footnote, t.color.tertiary, Align::End);
            let (title_rect, rest) = rest.take_top(f.line(TextRole::Headline), 0.0);
            f.text_in(title_rect, title, TextRole::Headline, t.color.label, Align::Start);
            f.paragraph(rest.left, rest.top, rest.width(), body, TextRole::Callout, t.color.secondary, 2);
            f.hit(id, card, accesskit::Role::Button, title, true, None);
        }
        stack.used()
    };
    f.scroll(viewport, &scroll, now, draw_list);
    scroll.scroll_by(f.px(90.0), false, now);
    // Draw again at the new offset: drop the first draw's ops (the clipped group and the
    // scrollbar) and hits.
    f.list.ops.truncate(f.list.ops.len() - 2);
    f.list.hits.retain(|h| !(60..70).contains(&h.id));
    f.scroll(viewport, &scroll, now, draw_list);
    // The month under the list stays put.
    let (month, days) = footer.take_top(f.px(28.0), f.px(4.0));
    f.text_in(month, "November 2026", TextRole::Title3, t.color.label, Align::Start);
    let (next, rest_month) = month.take_right(f.px(28.0), f.px(4.0));
    f.icon_button(14, next, Icon::ChevronRight, 28.0, "Next month", false);
    let (prev, _) = rest_month.take_right(f.px(28.0), 0.0);
    f.icon_button(15, prev, Icon::ChevronLeft, 28.0, "Previous month", false);
    for (i, d) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
        let cell = grid(days, 7, 0.0, f.px(22.0), i);
        f.text_in(cell, d, TextRole::Caption, t.color.tertiary, Align::Center);
    }
    for day in 1..=14u32 {
        let cell = grid(days.take_top(f.px(22.0), 0.0).1, 7, 0.0, f.px(28.0), (day + 5) as usize - 1);
        if day == 5 {
            let d = f.px(26.0);
            f.fill(cell.place(d, d, Align::Center, Align::Center), d / 2.0, t.color.accent);
            f.text_in(cell, "5", TextRole::Callout, t.color.on_accent, Align::Center);
        } else {
            f.text_in(cell, &day.to_string(), TextRole::Callout, t.color.label, Align::Center);
        }
    }

    // 3. Controls and states.
    let c = panel(792.0, 520.0);
    if let Some(g) = f.surface(c, true) {
        glass.push((g.body, g.radius));
    }
    let content = c.inset(Insets::all(pad));
    let mut col = Stack::column(content, f.px(t.metrics.gap));
    f.section_header(col.next(f.px(20.0)), "BUTTONS", Some((43, "Show all")));
    let row1 = col.next(f.px(t.metrics.control));
    let mut row = Stack::row(row1, f.px(8.0));
    let (w, _) = f.button_size("Install", false);
    f.button(44, row.next(w), "Install", None, ButtonKind::Filled, true);
    let (w, _) = f.button_size("Share", true);
    f.button(45, row.next(w), "Share", Some(Icon::Upload), ButtonKind::Tinted, true);
    let (w, _) = f.button_size("Cancel", false);
    f.button(46, row.next(w), "Cancel", None, ButtonKind::Gray, true);
    let row2 = col.next(f.px(t.metrics.control));
    let mut row = Stack::row(row2, f.px(8.0));
    let (w, _) = f.button_size("Delete", true);
    f.button(47, row.next(w), "Delete", Some(Icon::Delete), ButtonKind::Destructive, true);
    let (w, _) = f.button_size("Learn more", false);
    f.button(48, row.next(w), "Learn more", None, ButtonKind::Plain, true);
    let (w, _) = f.button_size("Disabled", false);
    f.button(49, row.next(w), "Disabled", None, ButtonKind::Filled, false);
    f.segmented(70, col.next(f.px(30.0)), &["Day", "Week", "Month"], 1);
    let field = col.next(f.px(34.0));
    f.field_frame(field, "Search apps, files and settings", true, true, Some(Icon::Search));
    let slider = col.next(f.px(24.0));
    f.slider(80, slider, 0.62, "Opacity", None);
    f.section_header(col.next(f.px(20.0)), "LIST", None);
    let list = col.next(f.px(t.metrics.row * 2.0 + t.metrics.row_single + 12.0));
    f.card(list);
    let mut rows = Stack::column(list.inset(Insets::all(f.px(6.0))), 0.0);
    let entries = [
        Row {
            id: 31,
            lead: Lead::Icon(Icon::Wifi(2)),
            title: "Home 5G",
            subtitle: Some("Connected, secured"),
            accessory: Accessory::Check,
            selected: false,
            enabled: true,
        },
        Row {
            id: 33,
            lead: Lead::Monogram('C'),
            title: "Café Lumen",
            subtitle: Some("Open"),
            accessory: Accessory::Chevron,
            selected: false,
            enabled: true,
        },
        Row {
            id: 34,
            lead: Lead::Icon(Icon::Speaker),
            title: "Speakers",
            subtitle: None,
            accessory: Accessory::Text("Default"),
            selected: false,
            enabled: false,
        },
    ];
    for entry in &entries {
        let h = f.row_height(entry);
        f.row(entry, rows.next(h));
    }
    let meters = col.next(f.px(56.0));
    let mut row = Stack::row(meters, f.px(14.0));
    for (i, v) in [0.32, 0.71, 0.9].into_iter().enumerate() {
        let r = row.next(f.px(56.0));
        let color = [None, Some(t.color.success), Some(t.color.warning)][i];
        f.ring(r.center(), f.px(22.0), f.px(5.0), v, color);
        f.text_in(r, &format!("{:.0}", v * 100.0), TextRole::Caption, t.color.label, Align::Center);
    }
    let rest = row.next(f.px(120.0));
    let x = f.badge(rest.left, rest.top + f.px(12.0), "3", Some(t.color.destructive));
    f.badge(rest.left + x + f.px(6.0), rest.top + f.px(12.0), "Beta", None);
    let tip_anchor = Rect::new(rest.left, rest.top + f.px(24.0), f.px(24.0), f.px(10.0));
    f.tooltip(tip_anchor, "Battery 82%", Rect::new(0.0, 0.0, W as f32, H as f32));

    let mut list = f.list;
    // A menu, from the same theme.
    let items: Vec<Entry<u32>> = vec![
        Entry::Action {
            label: "New folder".into(),
            command: 1,
            checked: None,
            enabled: true,
        },
        Entry::Action {
            label: "Show desktop icons".into(),
            command: 2,
            checked: Some(true),
            enabled: true,
        },
        Entry::Separator,
        Entry::Submenu {
            label: "Sort by".into(),
            items: vec![],
        },
        Entry::Action {
            label: "Personalize".into(),
            command: 3,
            checked: None,
            enabled: false,
        },
    ];
    let tokens = Theme::tokens(t);
    let layout = mypaper_ui::menu::layout(&items, &tokens, 1.0, &mut ui.text);
    let menu = mypaper_ui::menu::view(&items, &layout, Some(1), &tokens).translated(792.0, 556.0);
    if tokens.glass.is_some() {
        // The menu's body sits inside its shadow margin.
        let edge = if tokens.shadow { tokens.menu.shadow } else { 0.0 };
        let body = Rect::new(792.0 + edge, 556.0 + edge, layout.width, layout.height);
        glass.push((body, tokens.radius));
    }
    list.ops.extend(menu.ops);
    (list, glass)
}

/// The text lab: the same labels under each rasterization, on dark and light glass, with the
/// named static faces against the variable axes.
fn text_lab(ui: &mut Ui, out: &Path, backdrop: &Image, blurred: &Image) -> anyhow::Result<()> {
    let (w, h) = (1120u32, 520u32);
    let variants = [
        ("before: gamma 1.8, contrast 0.5 (ClearType field), static faces", TextQuality {
            gamma: 1.8,
            contrast: 0.5,
            mode: TextMode::NaturalSymmetric,
            grid_fit: true,
        }, false),
        ("after: gamma 1.8, grayscale contrast 1.0, variable axes", TextQuality::default(), true),
        ("contrast 1.5", TextQuality { contrast: 1.5, ..TextQuality::default() }, true),
        ("gamma 2.2", TextQuality { gamma: 2.2, ..TextQuality::default() }, true),
        ("GDI classic (hinted advances)", TextQuality { mode: TextMode::GdiClassic, ..TextQuality::default() }, true),
    ];
    let mut bg = backdrop_crop(backdrop, w, h);
    let blur_bg = backdrop_crop(blurred, w, h);
    for (i, scheme) in [Scheme::Dark, Scheme::Light].into_iter().enumerate() {
        let t = Theme::new(Style::Liquid, scheme, mypaper_ui::theme::SYSTEM_BLUE);
        glass_stand_in(&mut bg, &blur_bg, Rect::new(i as f32 * 560.0 + 10.0, 10.0, 540.0, 500.0), 24.0, &t);
    }
    let mut composite = bg.clone();
    for (row, (name, quality, variable)) in variants.iter().enumerate() {
        ui.set_text_quality(*quality)?;
        let mut list = DisplayList::default();
        for (i, scheme) in [Scheme::Dark, Scheme::Light].into_iter().enumerate() {
            let t = Theme::new(Style::Liquid, scheme, mypaper_ui::theme::SYSTEM_BLUE);
            let x = i as f32 * 560.0 + 30.0;
            let y = 24.0 + row as f32 * 96.0;
            let style = |role: TextRole| -> TextStyle {
                let s = t.font.style(role, 1.0);
                if *variable {
                    s
                } else {
                    let f = t.font.font(role);
                    let mut old = TextStyle::new(
                        if f.size > 20.0 { "Segoe UI Variable Display" } else { "Segoe UI Variable Text" },
                        f.size,
                    );
                    old.weight = f.weight;
                    old
                }
            };
            let put = |list: &mut DisplayList, ui: &mut Ui, x: f32, y: f32, text: &str, s: TextStyle, color: Rgba| {
                let (tw, th) = ui.text.measure(text, &s);
                list.ops.push(mypaper_ui::Op::Text {
                    rect: Rect::new(x, y, tw, th),
                    text: text.into(),
                    style: s,
                    color,
                });
                tw
            };
            put(&mut list, ui, x, y, name, style(TextRole::Caption), t.color.tertiary);
            put(&mut list, ui, x, y + 16.0, "Notifications", style(TextRole::Title2), t.color.label);
            let w1 = put(&mut list, ui, x + 150.0, y + 18.0, "Wi-Fi  Bluetooth  9:41", style(TextRole::Headline), t.color.label);
            put(&mut list, ui, x + 160.0 + w1, y + 20.0, "Medium 500", {
                let mut s = style(TextRole::Body);
                s.weight = 500;
                s
            }, t.color.label);
            put(&mut list, ui, x, y + 44.0, "The quick brown fox jumps over the lazy dog — 0123456789", style(TextRole::Body), t.color.label);
            put(&mut list, ui, x, y + 64.0, "Secondary: Connected, secured · 5 min ago", style(TextRole::Callout), t.color.secondary);
        }
        let image = render(ui, &list, w, h)?;
        // Keep only this row's band.
        let band = Rect::new(0.0, 20.0 + row as f32 * 96.0, w as f32, 96.0);
        let layer = over(&image, &composite);
        copy_band(&mut composite, &layer, band);
    }
    ui.set_text_quality(TextQuality::default())?;
    save(&out.join("kit-text.png"), &composite, 2)?;
    Ok(())
}

/// Vector icons against Segoe Fluent Icons, at three sizes, on dark and light glass.
fn icon_sheet(ui: &mut Ui, out: &Path, backdrop: &Image, blurred: &Image) -> anyhow::Result<()> {
    let icons = [
        Icon::Wifi(3), Icon::Bluetooth, Icon::Volume(2), Icon::Mute, Icon::Battery(7), Icon::Charging,
        Icon::Moon, Icon::Sun, Icon::Bell, Icon::BellOff, Icon::Settings, Icon::Power, Icon::Search,
        Icon::Apps, Icon::Play, Icon::Pause, Icon::Previous, Icon::Next, Icon::ChevronRight,
        Icon::Close, Icon::Add, Icon::Edit, Icon::Lock, Icon::Cloud, Icon::Rain, Icon::Thunder,
        Icon::Thermometer, Icon::Cpu, Icon::Calendar, Icon::Clock,
    ];
    let (w, h) = (1120u32, 300u32);
    let mut bg = backdrop_crop(backdrop, w, h);
    let blur_bg = backdrop_crop(blurred, w, h);
    let mut list = DisplayList::default();
    let family = mypaper_ui::icons::system_family(&ui.text);
    for (i, scheme) in [Scheme::Dark, Scheme::Light].into_iter().enumerate() {
        let x0 = i as f32 * 560.0 + 10.0;
        let vector = Theme::new(Style::Liquid, scheme, mypaper_ui::theme::SYSTEM_BLUE);
        glass_stand_in(&mut bg, &blur_bg, Rect::new(x0, 10.0, 540.0, 280.0), 24.0, &vector);
        let font = Theme::new(Style::Fluent, scheme, mypaper_ui::theme::SYSTEM_BLUE);
        let input = Interaction::default();
        for (row, (theme, label)) in [(&vector, "Lucide vectors"), (&font, "Segoe Fluent Icons")].into_iter().enumerate() {
            let mut f = Frame::new(&mut ui.text, theme, 1.0, &input);
            for (band, size) in [13.0f32, 16.0, 20.0].into_iter().enumerate() {
                let y = 30.0 + row as f32 * 130.0 + band as f32 * 34.0;
                f.label(x0 + 14.0, y - 2.0, &format!("{label} {size}"), TextRole::Caption, vector.color.tertiary, 110.0);
                for (j, icon) in icons.iter().take(15).enumerate() {
                    let cell = Rect::new(x0 + 130.0 + j as f32 * 27.0, y - 4.0, 26.0, 26.0);
                    // Font glyphs always, for the comparison.
                    if row == 1 {
                        let op = mypaper_ui::icons::centered(f.text, &family, *icon, size, cell, vector.color.label);
                        f.push(op);
                    } else {
                        f.icon(cell, *icon, size, vector.color.label);
                    }
                }
            }
            list.ops.extend(f.list.ops);
        }
    }
    let image = render(ui, &list, w, h)?;
    save(&out.join("kit-icons.png"), &over(&image, &bg), 2)?;
    Ok(())
}

// ---- Pixels -----------------------------------------------------------------------------

/// The engine's glass, roughly: the blurred wallpaper, the material's tint and one rim.
fn glass_stand_in(bg: &mut Image, blurred: &Image, rect: Rect, radius: f32, t: &Theme) {
    let Some(material) = t.glass else { return };
    let tint = material.tint;
    let alpha = (1.0 - material.clarity).clamp(0.0, 1.0) * 0.9 + 0.08;
    for y in rect.top as u32..(rect.bottom as u32).min(bg.height) {
        for x in rect.left as u32..(rect.right as u32).min(bg.width) {
            let d = rounded_distance(rect, radius, x as f32 + 0.5, y as f32 + 0.5);
            if d > 0.5 {
                continue;
            }
            let cover = (0.5 - d).clamp(0.0, 1.0);
            let rim = if d > -1.5 { 0.22 * (1.0 - (d + 1.5) / 2.0).clamp(0.0, 1.0) } else { 0.0 };
            let at = ((y * bg.width + x) * 4) as usize;
            for (k, channel) in [tint.b, tint.g, tint.r].into_iter().enumerate() {
                let b = f32::from(blurred.pixels[at + k]) / 255.0;
                let glass = b * (1.0 - alpha) + channel * alpha;
                let glass = glass + (1.0 - glass) * rim;
                let old = f32::from(bg.pixels[at + k]) / 255.0;
                bg.pixels[at + k] = ((old * (1.0 - cover) + glass * cover) * 255.0).round() as u8;
            }
        }
    }
}

fn rounded_distance(r: Rect, radius: f32, x: f32, y: f32) -> f32 {
    let cx = x.clamp(r.left + radius, r.right - radius);
    let cy = y.clamp(r.top + radius, r.bottom - radius);
    let (dx, dy) = (x - cx, y - cy);
    (dx * dx + dy * dy).sqrt() - radius
}

/// Premultiplied `top` over opaque `bg`.
fn over(top: &Image, bg: &Image) -> Image {
    let mut out = bg.clone();
    for (o, t) in out.pixels.as_chunks_mut::<4>().0.iter_mut().zip(top.pixels.as_chunks::<4>().0) {
        let a = f32::from(t[3]) / 255.0;
        for k in 0..3 {
            o[k] = (f32::from(t[k]) + f32::from(o[k]) * (1.0 - a)).round().min(255.0) as u8;
        }
        o[3] = 255;
    }
    out
}

fn copy_band(dst: &mut Image, src: &Image, band: Rect) {
    for y in band.top as u32..(band.bottom as u32).min(dst.height) {
        let at = (y * dst.width * 4) as usize;
        let len = (dst.width * 4) as usize;
        dst.pixels[at..at + len].copy_from_slice(&src.pixels[at..at + len]);
    }
}

fn cover(image: &Image, w: u32, h: u32) -> Image {
    let scale = (w as f32 / image.width as f32).max(h as f32 / image.height as f32);
    let (ox, oy) = (
        (image.width as f32 * scale - w as f32) / 2.0,
        (image.height as f32 * scale - h as f32) / 2.0,
    );
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let sx = (((x as f32 + ox) / scale) as u32).min(image.width - 1);
            let sy = (((y as f32 + oy) / scale) as u32).min(image.height - 1);
            let s = ((sy * image.width + sx) * 4) as usize;
            let d = ((y * w + x) * 4) as usize;
            pixels[d..d + 4].copy_from_slice(&image.pixels[s..s + 4]);
        }
    }
    Image { width: w, height: h, pixels }
}

fn backdrop_crop(image: &Image, w: u32, h: u32) -> Image {
    cover(image, w, h)
}

/// Three box blurs (close to a Gaussian), horizontal and vertical.
fn blur(image: &Image, radius: usize) -> Image {
    let (w, h) = (image.width as usize, image.height as usize);
    let mut a = image.pixels.clone();
    let mut b = a.clone();
    for _ in 0..3 {
        for pass in 0..2 {
            let (outer, inner, step_in, step_out) = if pass == 0 { (h, w, 4, w * 4) } else { (w, h, w * 4, 4) };
            for o in 0..outer {
                for k in 0..3 {
                    let at = |i: usize| o * step_out + i * step_in + k;
                    let mut sum = 0u32;
                    let n = (2 * radius + 1) as u32;
                    for i in 0..=2 * radius {
                        sum += u32::from(a[at(i.saturating_sub(radius).min(inner - 1))]);
                    }
                    for i in 0..inner {
                        b[at(i)] = (sum / n) as u8;
                        let add = (i + radius + 1).min(inner - 1);
                        let sub = i.saturating_sub(radius);
                        sum = sum + u32::from(a[at(add)]) - u32::from(a[at(sub)]);
                    }
                }
            }
            std::mem::swap(&mut a, &mut b);
        }
    }
    Image { width: image.width, height: image.height, pixels: a }
}

/// Writes `image` (BGRA) as an RGB PNG, scaled up `zoom` times without smoothing.
fn save(path: &Path, image: &Image, zoom: u32) -> anyhow::Result<()> {
    let (w, h) = (image.width * zoom, image.height * zoom);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let p = image.pixel(x / zoom, y / zoom);
            rgb.extend_from_slice(&[p[2], p[1], p[0]]);
        }
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgb)?;
    Ok(())
}
