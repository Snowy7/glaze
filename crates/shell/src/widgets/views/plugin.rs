//! SDK plugin faces (docs/plugins.md, "Views"): the host draws a plugin's declarative view tree
//! with the kit, inside the standard integration chrome. Pure: no plugin I/O here. Actions are
//! numbered in tree order, so a click maps back to its action id with [`action_at`].
use mypaper_plugin_sdk::{Align as VAlign, Face, Role as VRole, Tone, View};
use mypaper_ui::chart::{LimitBar, Series, area_chart, compact_limit_bar};
use mypaper_ui::kit::{Accessory, ButtonKind, Frame, Lead, Row};
use mypaper_ui::layout::{Align, Insets, SizeClass};
use mypaper_ui::{HitId, Point, Rect, Rgba, icons::Icon, theme::TextRole};

pub mod animated;
pub mod canvas;

/// Hit ids of a face's actions: `ACTION + n` for the nth action in tree order.
pub const ACTION: HitId = 800_000;
const MAX_ACTIONS: HitId = 10_000;
/// The header's refresh button (`Face::refresh`).
const REFRESH: HitId = ACTION - 1;

/// How the face is doing, shown in the footer when it isn't live.
pub enum Status<'a> {
    Live,
    Starting,
    Restarting,
    Failed(&'a str),
}

/// What a hit in a face asks for: a plain action, or text entry for a `field`'s action.
#[derive(Debug, PartialEq)]
pub enum Hit {
    Action(String),
    Field(String),
    Select { action: String, options: Vec<mypaper_plugin_sdk::view::SelectOption>, selected: Option<String> },
}

/// The plugin action behind hit `hit` in `face`, if any.
pub fn action_at(face: &Face, hit: HitId) -> Option<Hit> {
    if hit == REFRESH && face.refresh {
        return Some(Hit::Action("refresh".into()));
    }
    let n = hit.checked_sub(ACTION).filter(|n| *n < MAX_ACTIONS)? as usize;
    let mut actions = Vec::new();
    collect(&face.body, &mut actions);
    actions.into_iter().nth(n)
}

fn collect(view: &View, out: &mut Vec<Hit>) {
    match view {
        View::Stack { children, .. } | View::List { children, .. } | View::Row { children, .. } | View::Grid { children, .. } => {
            children.iter().for_each(|c| collect(c, out))
        }
        View::Card { child, .. } | View::Animated { child, .. } | View::Scroll { child, .. } => collect(child, out),
        View::Button { action, .. } | View::IconButton { action, .. } => out.push(Hit::Action(action.clone())),
        View::Field { action, .. } => out.push(Hit::Field(action.clone())),
        View::Select { action, options, selected, .. } => out.push(Hit::Select { action: action.clone(), options: options.clone(), selected: selected.clone() }),
        View::ListRow { action: Some(action), .. } | View::Item { action: Some(action), .. } => out.push(Hit::Action(action.clone())),
        _ => {}
    }
}

/// A kit icon by its SDK name (lowercase, `-` between words); unknown names draw a dot.
pub fn icon(name: &str) -> Icon {
    match name {
        "terminal" => Icon::Terminal,
        "gauge" => Icon::Gauge,
        "activity" => Icon::Activity,
        "bell" => Icon::Bell,
        "check" => Icon::Check,
        "close" => Icon::Close,
        "add" => Icon::Add,
        "refresh" => Icon::Refresh,
        "message" => Icon::Message,
        "warning" => Icon::Warning,
        "info" => Icon::Info,
        "clock" => Icon::Clock,
        "calendar" => Icon::Calendar,
        "globe" => Icon::Globe,
        "package" => Icon::Package,
        "video" => Icon::Video,
        "sparkles" => Icon::Sparkles,
        "rocket" => Icon::Rocket,
        "folder" => Icon::Folder,
        "user" => Icon::User,
        "layers" => Icon::Layers,
        "external" => Icon::External,
        "chevron-right" => Icon::ChevronRight,
        "chevron-left" => Icon::ChevronLeft,
        "chevron-up" => Icon::ChevronUp,
        "chevron-down" => Icon::ChevronDown,
        "upload" => Icon::Upload,
        "download" => Icon::Download,
        "edit" => Icon::Edit,
        "camera" => Icon::Camera,
        "more" => Icon::More,
        "play" => Icon::Play,
        "pause" => Icon::Pause,
        "tasks" => Icon::Tasks,
        "sun" => Icon::Sun,
        "moon" => Icon::Moon,
        "cloud" => Icon::Cloud,
        "cloud-sun" => Icon::CloudSun,
        "cloud-moon" => Icon::CloudMoon,
        "cloud-rain" => Icon::Rain,
        "cloud-drizzle" => Icon::Drizzle,
        "cloud-snow" => Icon::Snow,
        "cloud-lightning" => Icon::Thunder,
        "cloud-fog" => Icon::Fog,
        "snowflake" => Icon::Snowflake,
        "thermometer" => Icon::Thermometer,
        "map-pin" => Icon::Location,
        "cpu" => Icon::Cpu,
        "star" => Icon::Star,
        "heart" => Icon::Heart,
        "mic" => Icon::Mic,
        "headphones" => Icon::Headphones,
        "settings" => Icon::Settings,
        _ => Icon::More,
    }
}

pub(crate) fn role(role: VRole) -> TextRole {
    match role {
        VRole::LargeTitle => TextRole::LargeTitle,
        VRole::Title1 => TextRole::Title1,
        VRole::Title2 => TextRole::Title2,
        VRole::Title3 => TextRole::Title3,
        VRole::Headline => TextRole::Headline,
        VRole::Body => TextRole::Body,
        VRole::Callout => TextRole::Callout,
        VRole::Subheadline => TextRole::Subheadline,
        VRole::Footnote => TextRole::Footnote,
        VRole::Caption => TextRole::Caption,
    }
}

pub(crate) fn tone(f: &Frame<'_>, tone: Option<&Tone>, default: Rgba) -> Rgba {
    let c = f.theme.color;
    match tone {
        None => default,
        Some(Tone::Label) => c.label,
        Some(Tone::Secondary) => c.secondary,
        Some(Tone::Tertiary) => c.tertiary,
        Some(Tone::Accent) => c.accent,
        Some(Tone::Warning) => c.warning,
        Some(Tone::Destructive) => c.destructive,
        Some(Tone::Positive) => c.success,
        Some(Tone::Hex(hex)) => {
            let hex = hex.trim_start_matches('#');
            match (0..3).map(|i| hex.get(i * 2..i * 2 + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect::<Option<Vec<u8>>>() {
                Some(rgb) if hex.len() == 6 => Rgba::rgb8([rgb[0], rgb[1], rgb[2]], 1.0),
                _ => default,
            }
        }
    }
}

/// Provider identities share their readable brand ink across chart lines and legend dots.
/// Quota bars keep the plugin's darker track fill.
fn brand_ink(f: &Frame<'_>, name: &str) -> Option<Rgba> {
    let name = name.to_ascii_lowercase();
    match name.as_str() {
        "claude" | "claude code" | "claudeagent" | "claude-agent" => Some(Rgba::rgb8([217, 119, 87], 1.0)),
        "codex" | "openai codex" => Some(f.theme.color.label),
        _ => None,
    }
}

/// The face with its chrome: icon + title header, body, footer caption.
pub fn draw(f: &mut Frame<'_>, rect: Rect, title: &str, header: Icon, face: Option<&Face>, status: Status<'_>) {
    let compact = SizeClass::of(rect.width() / f.s, rect.height() / f.s) == SizeClass::Compact;
    let inner = rect.inset(Insets::all(f.px(if compact { 12.0 } else { 16.0 })));
    let (head, mut body) = inner.take_top(f.px(22.0), f.px(8.0));
    let (symbol, label) = head.take_left(f.px(17.0), f.px(6.0));
    f.icon(symbol, header, 16.0, f.theme.color.accent);
    let label = if face.is_some_and(|face| face.refresh) {
        let (control, label) = label.take_right(f.px(22.0), f.px(3.0));
        f.icon_button(REFRESH, control, Icon::Refresh, 15.0, "Retry or refresh", false);
        label
    } else {
        label
    };
    f.text_in(label, title, TextRole::Subheadline, f.theme.color.secondary, Align::Start);
    let footer = (rect.height() / f.s >= 170.0).then(|| {
        let (footer, rest) = body.take_bottom(f.px(16.0), f.px(6.0));
        body = rest;
        footer
    });
    let note = match status {
        Status::Live => face.and_then(|face| face.footer.as_deref()),
        Status::Starting => Some("Starting…"),
        Status::Restarting => Some("Restarting · showing last state"),
        Status::Failed(why) => Some(why),
    };
    if let (Some(footer), Some(note)) = (footer, note) {
        let color = match status {
            Status::Restarting => f.theme.color.warning,
            _ => f.theme.color.tertiary,
        };
        f.text_in(footer, note, TextRole::Caption, color, Align::Start);
    }
    match face {
        Some(face) => {
            let mut next = ACTION;
            node(f, body, &face.body, &mut next);
        }
        None => {
            empty(f, body, Some(header), "Starting…", None);
        }
    }
}

/// Empty states give up their artwork and detail before they paint outside a short tile.
fn empty(f: &mut Frame<'_>, rect: Rect, icon: Option<Icon>, title: &str, detail: Option<&str>) {
    let head = f.px(12.0) + icon.map_or(0.0, |_| f.px(38.0)) + f.line(TextRole::Headline);
    if rect.height() < head {
        let (_, height) = f.text.measure(title, &f.font(TextRole::Headline));
        if rect.height() >= height {
            f.text_in(rect, title, TextRole::Headline, f.theme.color.secondary, Align::Center);
        }
        return;
    }
    let detail = detail.filter(|detail| {
        let lines = f.text.lines(detail, &f.font(TextRole::Callout), rect.width(), 3).len();
        head + f.px(2.0) + lines as f32 * f.line(TextRole::Callout) <= rect.height()
    });
    f.empty(rect, icon, title, detail);
}

/// Whether a node takes the free space of its stack (charts, empties, spacers).
fn flexible(view: &View) -> bool {
    match view {
        View::Animated { child, .. } => flexible(child),
        _ => matches!(view, View::Spacer | View::AreaChart { .. } | View::Empty { .. } | View::List { .. } | View::Scroll { .. }),
    }
}

/// A node's natural height at `width`.
fn height(f: &mut Frame<'_>, view: &View, width: f32) -> f32 {
    match view {
        View::Stack { children, gap } => {
            let gap = f.px(gap.unwrap_or(8.0));
            let sum: f32 = children.iter().map(|c| height(f, c, width)).sum();
            sum + gap * children.len().saturating_sub(1) as f32
        }
        View::Row { children, .. } => children.iter().map(|c| height(f, c, width)).fold(0.0, f32::max),
        View::Grid { children, columns, gap } => {
            let columns = (*columns).max(1) as usize;
            let gap = f.px(gap.unwrap_or(8.0));
            let cell = (width - gap * (columns - 1) as f32) / columns as f32;
            let rows: Vec<f32> = children
                .chunks(columns)
                .map(|row| row.iter().map(|c| height(f, c, cell)).fold(0.0, f32::max))
                .collect();
            rows.iter().sum::<f32>() + gap * rows.len().saturating_sub(1) as f32
        }
        View::Card { title, child } => {
            let pad = f.px(f.theme.metrics.pad) * 2.0;
            let header = title.as_ref().map_or(0.0, |_| f.line(TextRole::Subheadline) + f.px(6.0));
            header + height(f, child, width - pad) + pad
        }
        View::Text { role: r, .. } => f.line(role(*r)),
        View::Icon { size, .. } => f.px(size.unwrap_or(16.0)),
        View::Button { .. } | View::Field { .. } | View::Select { .. } => f.px(f.theme.metrics.control),
        View::ListRow { detail, .. } => {
            let row = Row {
                id: 0,
                lead: Lead::None,
                title: "",
                subtitle: detail.as_deref(),
                accessory: Accessory::None,
                selected: false,
                enabled: true,
            };
            f.row_height(&row)
        }
        View::Stat { detail, .. } => {
            f.line(TextRole::Caption) + f.line(TextRole::Title2) + detail.as_ref().map_or(0.0, |_| f.line(TextRole::Caption))
        }
        View::Metric { .. } => f.px(36.0) + f.line(TextRole::Caption),
        View::Tag { .. } => f.line(TextRole::Caption) + f.px(4.0),
        View::IconButton { .. } => f.px(28.0),
        View::Ring { .. } => f.px(48.0),
        View::LimitBar { .. } => f.px(30.0),
        View::SegmentedBar { .. } => f.px(8.0),
        View::Sparkline { .. } => f.px(28.0),
        View::AreaChart { .. } | View::Empty { .. } | View::List { .. } => f.px(80.0),
        View::Canvas { height, .. } => f.px(height.clamp(0.0, 2048.0)),
        View::Meter { .. } => f.line(TextRole::Title2) + f.px(13.0),
        View::Item { detail, nested, .. } => match (detail, nested) {
            (_, true) => f.line(TextRole::Callout) + f.px(8.0),
            (Some(_), false) => f.line(TextRole::Headline) + f.line(TextRole::Footnote) + f.px(12.0),
            (None, false) => f.line(TextRole::Headline) + f.px(12.0),
        },
        View::Animated { child, .. } | View::Scroll { child, .. } => self::height(f, child, width),
        View::Spacer | View::Unknown => 0.0,
    }
}

fn node(f: &mut Frame<'_>, rect: Rect, view: &View, next: &mut HitId) {
    let mut take_hit = || {
        let id = *next;
        *next += 1;
        id
    };
    match view {
        // Until this renderer has local scroll state, use the child's bounded layout. Keep
        // action traversal identical to the SDK tree, including children of a scroll view.
        View::Scroll { child, .. } => node(f, rect, child, next),
        View::Stack { children, gap } | View::List { children, gap } => {
            let gap = f.px(gap.unwrap_or(8.0));
            let fixed: f32 = children.iter().filter(|c| !flexible(c)).map(|c| height(f, c, rect.width())).sum();
            let flex = children.iter().filter(|c| flexible(c)).count();
            let spare = (rect.height() - fixed - gap * children.len().saturating_sub(1) as f32).max(0.0);
            let mut top = rect.top;
            for child in children {
                let h = if flexible(child) { spare / flex.max(1) as f32 } else { height(f, child, rect.width()) };
                // What doesn't fit isn't drawn (no clipped half rows over the footer).
                if !flexible(child) && top + h > rect.bottom + 1.0 {
                    break;
                }
                node(f, Rect::new(rect.left, top, rect.width(), h), child, next);
                top += h + gap;
            }
        }
        View::Row { children, gap } => {
            let gap = f.px(gap.unwrap_or(8.0));
            // A kind this shell doesn't know draws nothing, so it takes no room either: a newer
            // plugin's status tag must not halve its row's title (2026-11-06, T3 "Running now").
            let children: Vec<&View> = children.iter().filter(|c| !matches!(c, View::Unknown)).collect();
            // Icons and buttons keep their natural width; the rest share what's left.
            let natural = |f: &mut Frame<'_>, c: &View| match c {
                View::Icon { size, .. } => Some(f.px(size.unwrap_or(16.0))),
                View::Button { label, icon: i, .. } => Some(f.button_size(label, i.is_some()).0),
                View::IconButton { .. } => Some(f.px(28.0)),
                View::Metric { width: Some(width), .. } => Some(f.px(*width)),
                View::Tag { label, dot, .. } => Some(tag_width(f, label, *dot)),
                View::Spacer => None,
                _ => None,
            };
            let widths: Vec<Option<f32>> = children.iter().map(|c| natural(f, c)).collect();
            let fixed: f32 = widths.iter().flatten().sum();
            let shared = widths.iter().filter(|w| w.is_none()).count().max(1);
            let spare = (rect.width() - fixed - gap * children.len().saturating_sub(1) as f32).max(0.0) / shared as f32;
            let mut left = rect.left;
            for (child, w) in children.iter().zip(widths) {
                let w = w.unwrap_or(spare);
                node(f, Rect::new(left, rect.top, w, rect.height()), child, next);
                left += w + gap;
            }
        }
        View::Grid { children, columns, gap } => {
            let columns = (*columns).max(1) as usize;
            let gap = f.px(gap.unwrap_or(8.0));
            let cell = (rect.width() - gap * (columns - 1) as f32) / columns as f32;
            let mut top = rect.top;
            for row in children.chunks(columns) {
                let h = row.iter().map(|c| height(f, c, cell)).fold(0.0, f32::max);
                if top + h > rect.bottom + 1.0 {
                    break;
                }
                for (i, child) in row.iter().enumerate() {
                    node(f, Rect::new(rect.left + i as f32 * (cell + gap), top, cell, h), child, next);
                }
                top += h + gap;
            }
        }
        View::Card { title, child } => {
            let inner = f.card(rect);
            let inner = match title {
                Some(title) => {
                    let (head, rest) = inner.take_top(f.line(TextRole::Subheadline), f.px(6.0));
                    f.text_in(head, title, TextRole::Subheadline, f.theme.color.secondary, Align::Start);
                    rest
                }
                None => inner,
            };
            node(f, inner, child, next);
        }
        View::Text { text, role: r, color, align } => {
            let align = match align {
                VAlign::Start => Align::Start,
                VAlign::Center => Align::Center,
                VAlign::End => Align::End,
            };
            let ink = tone(f, color.as_ref(), f.theme.color.label);
            f.text_in(rect, text, role(*r), ink, align);
        }
        View::Icon { icon: name, size, color } => {
            let ink = tone(f, color.as_ref(), f.theme.color.secondary);
            if name == "dot" {
                // A status dot: a filled circle, centred.
                let d = f.px(size.unwrap_or(8.0));
                f.fill(rect.place(d, d, Align::Center, Align::Center), d / 2.0, ink);
            } else {
                f.icon(rect, icon(name), size.unwrap_or(16.0), ink);
            }
        }
        View::Button { label, icon: i, primary, .. } => {
            let kind = if *primary { ButtonKind::Filled } else { ButtonKind::Gray };
            let id = take_hit();
            f.button(id, rect, label, i.as_deref().map(icon), kind, true);
        }
        View::Select { label, options, selected, .. } => {
            let id = take_hit();
            let caption = options.iter().find(|option| Some(&option.value) == selected.as_ref()).map_or(label.as_str(), |option| option.label.as_str());
            f.button(id, rect, caption, Some(Icon::ChevronDown), ButtonKind::Gray, !options.is_empty());
        }
        View::Field { placeholder, .. } => {
            // Text entry goes through the host's RichEdit field (docs/plugins.md); drawn as its well.
            let id = take_hit();
            f.field_frame(rect, placeholder, true, f.input.ring(id), None);
            f.focus_ring(id, rect, f.control_radius(rect.height()));
            f.hit(id, rect, mypaper_ui::Role::TextInput, placeholder, true, None);
        }
        View::ListRow { title, detail, icon: i, trailing, action } => {
            let id = if action.is_some() { take_hit() } else { 0 };
            let rect = if i.is_none() && let Some(ink) = brand_ink(f, title) {
                let (dot, rest) = rect.take_left(f.px(10.0), f.px(2.0));
                let d = f.px(7.0);
                f.fill(dot.place(d, d, Align::Center, Align::Center), d / 2.0, ink);
                rest
            } else { rect };
            let row = Row {
                id,
                lead: i.as_deref().map_or(Lead::None, |name| Lead::Icon(icon(name))),
                title,
                subtitle: detail.as_deref(),
                accessory: match (trailing, action) {
                    (Some(text), _) => Accessory::Text(text),
                    (None, Some(_)) => Accessory::Chevron,
                    _ => Accessory::None,
                },
                selected: false,
                enabled: true,
            };
            f.row(&row, rect);
        }
        View::Stat { label, value, detail } => {
            let (head, rest) = rect.take_top(f.line(TextRole::Caption), 0.0);
            f.text_in(head, label, TextRole::Caption, f.theme.color.secondary, Align::Start);
            let (big, rest) = rest.take_top(f.line(TextRole::Title2), 0.0);
            f.text_in(big, value, TextRole::Title2, f.theme.color.label, Align::Start);
            if let Some(detail) = detail {
                f.text_in(rest, detail, TextRole::Caption, f.theme.color.tertiary, Align::Start);
            }
        }
        View::Metric { value, label, color, .. } => {
            let (number, caption) = rect.take_top(f.px(35.0), f.px(1.0));
            let ink = tone(f, color.as_ref(), f.theme.color.label);
            f.text_in(number, value, TextRole::LargeTitle, ink, Align::Start);
            f.text_in(caption, label, TextRole::Caption, f.theme.color.secondary, Align::Start);
        }
        View::IconButton { icon: name, label, enabled, .. } => {
            let id = take_hit();
            let control = rect.place(f.px(28.0), f.px(28.0), Align::Center, Align::Center);
            let radius = control.width() / 2.0;
            f.wash(id, control, radius, *enabled);
            let ink = if *enabled { f.theme.color.label } else { f.theme.color.quaternary };
            f.icon(control, icon(name), 16.0, ink);
            f.focus_ring(id, control, radius);
            f.hit(id, control, mypaper_ui::Role::Button, label, *enabled, None);
        }
        View::Ring { fraction, label, color } => {
            let radius = rect.height().min(rect.width()) / 2.0 - f.px(3.0);
            let center = Point::new(rect.left + rect.height().min(rect.width()) / 2.0, rect.top + rect.height() / 2.0);
            let ink = tone(f, color.as_ref(), f.theme.color.accent);
            f.ring(center, radius, f.px(5.0), fraction.clamp(0.0, 1.0), Some(ink));
            if let Some(label) = label {
                let r = Rect::new(center.x - radius, rect.top, radius * 2.0, rect.height());
                f.text_in(r, label, TextRole::Caption, f.theme.color.label, Align::Center);
            }
        }
        View::LimitBar { left, pace, color, badge, label, reset } => {
            let fill = tone(f, color.as_ref(), f.theme.color.accent);
            let bar = LimitBar {
                left: left.clamp(0.0, 1.0),
                pace: *pace,
                fill,
                badge: badge.as_deref(),
                label: label.as_deref(),
                reset: reset.as_deref(),
            };
            compact_limit_bar(f, rect, &bar);
        }
        View::SegmentedBar { segments } => {
            let radius = rect.height() / 2.0;
            f.fill(rect, radius, f.theme.color.well);
            let mut left = rect.left;
            for segment in segments {
                let w = rect.width() * segment.fraction.clamp(0.0, 1.0);
                let ink = tone(f, Some(&segment.color), f.theme.color.accent);
                f.fill(Rect::new(left, rect.top, w, rect.height()), radius, ink);
                left += w;
            }
        }
        View::Sparkline { values, color } => {
            let ink = tone(f, color.as_ref(), f.theme.color.accent);
            let max = values.iter().copied().fold(0.0_f32, f32::max).max(f32::EPSILON);
            let n = values.len().max(1) as f32;
            let w = rect.width() / n;
            for (i, v) in values.iter().enumerate() {
                let h = (rect.height() * (v / max)).max(f.px(1.0));
                f.fill(Rect::new(rect.left + i as f32 * w, rect.bottom - h, (w - f.px(1.0)).max(1.0), h), 0.0, ink);
            }
        }
        View::AreaChart { series, x_labels, unit } => {
            let values: Vec<Vec<f64>> = series.iter().map(|s| s.values.iter().map(|v| *v as f64).collect()).collect();
            let kit: Vec<Series<'_>> = series
                .iter()
                .zip(&values)
                .map(|(s, v)| Series { color: brand_ink(f, &s.label).unwrap_or_else(|| tone(f, s.color.as_ref(), f.theme.color.accent)), values: v })
                .collect();
            let unit = unit.clone().unwrap_or_default();
            let labels = [x_labels[0].as_str(), x_labels[1].as_str(), x_labels[2].as_str()];
            area_chart(f, rect, &kit, labels, |v| format!("{}{unit}", compact_number(v)));
        }
        View::Empty { icon: i, title, detail } => {
            empty(f, rect, i.as_deref().map(icon), title, detail.as_deref());
        }
        View::Canvas { commands, .. } => canvas::draw(f, rect, commands, |f, t| tone(f, Some(t), f.theme.color.label)),
        View::Animated { stiffness, damping, child } => animated::draw(f, rect, child, (*stiffness, *damping), next),
        View::Tag { label, color, dot } => {
            let ink = tone(f, Some(color), f.theme.color.secondary);
            let w = tag_width(f, label, *dot).min(rect.width());
            if *dot {
                let d = f.px(7.0);
                f.fill(rect.place(d, d, Align::Center, Align::Center), d / 2.0, ink);
            } else {
                let pill = rect.place(w, f.line(TextRole::Caption) + f.px(4.0), Align::End, Align::Center);
                f.fill(pill, pill.height() / 2.0, ink.with_alpha(0.16));
                let pad = f.px(7.0);
                let inner = Rect::new(pill.left + pad, pill.top, (pill.width() - pad * 2.0).max(0.0), pill.height());
                f.text_in(inner, label, TextRole::Caption, ink, Align::Center);
            }
        }
        View::Meter { value, label, note, fraction, color, mark } => {
            let c = f.theme.color;
            let ink = tone(f, color.as_ref(), c.label);
            let (line, rest) = rect.take_top(f.line(TextRole::Title2), f.px(5.0));
            let (vw, _) = f.text.measure(value, &f.font(TextRole::Title2));
            let (number, after) = line.take_left(vw.ceil().min(line.width()), f.px(8.0));
            f.text_in(number, value, TextRole::Title2, ink, Align::Start);
            // The label and note sit on the number's baseline. On a narrow tile the note goes
            // first, then the label, rather than either being cut to a stub.
            let small = f.line(TextRole::Footnote);
            let row = Rect::new(after.left, line.bottom - small - f.px(1.0), after.width().max(0.0), small);
            let nw = note.as_deref().map_or(0.0, |n| f.text.measure(n, &f.font(TextRole::Footnote)).0.ceil());
            let row = match note {
                Some(note) if nw + f.px(8.0 + 36.0) <= row.width() => {
                    let (end, start) = row.take_right(nw, f.px(8.0));
                    f.text_in(end, note, TextRole::Footnote, c.tertiary, Align::End);
                    start
                }
                _ => row,
            };
            if row.width() >= f.px(36.0) {
                f.text_in(row, label, TextRole::Footnote, c.secondary, Align::Start);
            }
            let bar = Rect::new(rect.left, rest.top + f.px(1.0), rect.width(), f.px(6.0));
            f.progress(bar, fraction.clamp(0.0, 1.0), Some(ink));
            if let Some(mark) = mark.filter(|m| (0.0..=1.0).contains(m)) {
                let x = (bar.left + bar.width() * mark).round();
                f.fill(Rect::new(x - f.px(1.0), bar.top - f.px(2.0), f.px(2.0), bar.height() + f.px(4.0)), f.px(1.0), c.tertiary);
            }
        }
        View::Item { title, detail, tag, tone: tag_tone, action, nested } => {
            let c = f.theme.color;
            let id = if action.is_some() { take_hit() } else { 0 };
            let radius = f.px(f.theme.radius.small);
            f.wash(id, rect, radius, true);
            let inner = rect.inset(Insets::xy(f.px(8.0), 0.0));
            let inner = if *nested { inner.take_left(f.px(14.0), 0.0).1 } else { inner };
            let room = match tag {
                // A narrow row keeps its title and shows the state as a dot.
                Some(_) if inner.width() < f.px(220.0) => {
                    let ink = tone(f, tag_tone.as_ref(), c.secondary);
                    let (end, room) = inner.take_right(f.px(8.0), f.px(8.0));
                    let d = f.px(7.0);
                    f.fill(end.place(d, d, Align::Center, Align::Center), d / 2.0, ink);
                    room
                }
                Some(label) => {
                    let ink = tone(f, tag_tone.as_ref(), c.secondary);
                    let w = tag_width(f, label, false).min(inner.width() * 0.5);
                    let (end, room) = inner.take_right(w, f.px(8.0));
                    let pill = end.place(w, f.line(TextRole::Caption) + f.px(4.0), Align::End, Align::Center);
                    f.fill(pill, pill.height() / 2.0, ink.with_alpha(0.16));
                    let pad = f.px(7.0);
                    let text = Rect::new(pill.left + pad, pill.top, (pill.width() - pad * 2.0).max(0.0), pill.height());
                    f.text_in(text, label, TextRole::Caption, ink, Align::Center);
                    room
                }
                None => inner,
            };
            let (role, ink) = if *nested { (TextRole::Callout, c.secondary) } else { (TextRole::Headline, c.label) };
            match detail.as_deref().filter(|_| !*nested) {
                Some(detail) => {
                    let (th, dh) = (f.line(role), f.line(TextRole::Footnote));
                    let top = (room.top + (room.height() - th - dh) / 2.0).round();
                    f.text_in(Rect::new(room.left, top, room.width(), th), title, role, ink, Align::Start);
                    f.text_in(Rect::new(room.left, top + th, room.width(), dh), detail, TextRole::Footnote, c.secondary, Align::Start);
                }
                None => {
                    f.text_in(room, title, role, ink, Align::Start);
                }
            }
            f.focus_ring(id, rect, radius);
            let name = match (detail, tag) {
                (Some(d), Some(t)) => format!("{title}, {d}, {t}"),
                (Some(d), None) => format!("{title}, {d}"),
                (None, Some(t)) => format!("{title}, {t}"),
                (None, None) => title.clone(),
            };
            f.hit(id, rect, mypaper_ui::Role::Button, &name, true, None);
        }
        View::Spacer | View::Unknown => {}
    }
}

/// A tag's natural width: a dot, or its caption text plus the pill's padding.
fn tag_width(f: &mut Frame<'_>, label: &str, dot: bool) -> f32 {
    if dot {
        return f.px(10.0);
    }
    let style = f.font(TextRole::Caption);
    // 7 pt each side, plus slack so fitting never cuts the label it was measured for.
    f.text.measure(label, &style).0.ceil() + f.px(18.0)
}

/// `1.2k`, `3.4M`: chart ticks.
fn compact_number(v: f64) -> String {
    match v.abs() {
        a if a >= 1e9 => format!("{:.1}B", v / 1e9),
        a if a >= 1e6 => format!("{:.1}M", v / 1e6),
        a if a >= 1e3 => format!("{:.0}k", v / 1e3),
        _ => format!("{v:.0}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_ui::{Interaction, Op, Scheme, Style, Theme, Ui};

    #[test]
    fn starting_faces_fit_short_tiles_without_an_empty_state_icon() {
        let mut ui = Ui::new().unwrap();
        let theme = Theme::new(Style::Liquid, Scheme::Dark, Rgba::rgb8([10, 132, 255], 1.0));
        let input = Interaction::default();
        for scale in [1.0, 1.5, 2.0] {
            for width in [75.0, 96.0, 180.0, 720.0] {
                for height in [75.0, 96.0, 132.0] {
                    let rect = Rect::new(0.0, 0.0, width * scale, height * scale);
                    let mut f = Frame::new(&mut ui.text, &theme, scale, &input);
                    draw(&mut f, rect, "Hello TypeScript", Icon::Sparkles, None, Status::Starting);
                    for op in &f.list.ops {
                        let bounds = match op { Op::Text { rect, .. } | Op::Icon { rect, .. } => *rect, _ => continue };
                        assert!(bounds.left >= -1.0 && bounds.top >= -1.0 && bounds.right <= rect.right + 1.0 && bounds.bottom <= rect.bottom + 1.0, "{width}x{height}, scale {scale}: {op:?}");
                    }
                    if height <= 96.0 {
                        assert_eq!(f.list.ops.iter().filter(|op| matches!(op, Op::Icon { .. })).count(), 1, "short tiles keep only the header icon");
                    }
                }
            }
        }
    }

    /// "Running now" rows span the widget at 100 % and 150 %, with their status pills at the
    /// right edge; a kind the shell doesn't know (a newer plugin's) takes no room.
    #[test]
    fn running_rows_fill_the_widget_at_any_scale() {
        let face = |pill: &str| {
            let row = |n: usize| serde_json::json!({"kind": "row", "gap": 6, "children": [
                {"kind": "list_row", "title": format!("Improve the App Store listing {n}"), "detail": "claude-opus-5-5", "icon": "terminal", "trailing": "", "action": format!("thread:{n}")},
                {"kind": pill, "label": "Working", "color": "#7dd3fc", "dot": false},
            ]});
            let face = serde_json::json!({"body": {"kind": "stack", "gap": 6, "children": [row(0), row(1), row(2)]}, "footer": "Live from your local T3 Code"});
            serde_json::from_value::<Face>(face).unwrap()
        };
        let (known, future) = (face("tag"), face("future-pill"));
        let mut ui = Ui::new().unwrap();
        let theme = Theme::new(Style::Liquid, Scheme::Dark, Rgba::rgb8([10, 132, 255], 1.0));
        let input = Interaction::default();
        for scale in [1.0, 1.5] {
            for (face, pills) in [(&known, true), (&future, false)] {
                let rect = Rect::new(0.0, 0.0, 420.0 * scale, 300.0 * scale);
                let mut f = Frame::new(&mut ui.text, &theme, scale, &input);
                draw(&mut f, rect, "T3 · Running now", Icon::Terminal, Some(face), Status::Live);
                let rows: Vec<_> = f.list.hits.iter().filter(|h| h.id >= ACTION).collect();
                assert!(rows.len() >= 3, "scale {scale}: {} rows", rows.len());
                let least = if pills { 0.55 } else { 0.85 };
                for row in rows {
                    assert!(row.rect.width() >= rect.width() * least, "scale {scale}, pills {pills}: row {:?}", row.rect);
                }
                let pill = f.list.ops.iter().find_map(|op| match op { Op::Text { rect, text, .. } if text.as_ref() == "Working" => Some(*rect), _ => None });
                match pill {
                    Some(pill) => assert!(pills && pill.right >= rect.width() * 0.85, "scale {scale}: pill {pill:?}"),
                    None => assert!(!pills, "scale {scale}: no status pill"),
                }
            }
        }
    }

    #[test]
    fn fields_use_the_kit_well_and_keep_their_action() {
        let mut ui = Ui::new().unwrap();
        let theme = Theme::new(Style::Liquid, Scheme::Dark, Rgba::rgb8([10, 132, 255], 1.0));
        let input = Interaction::default();
        let mut f = Frame::new(&mut ui.text, &theme, 1.0, &input);
        let field = View::Field { placeholder: "What should we build?".into(), action: "send".into() };
        let rect = Rect::new(0.0, 0.0, 300.0, 32.0);
        let mut next = ACTION;
        node(&mut f, rect, &field, &mut next);
        assert!(f.list.ops.iter().any(|op| matches!(op, Op::Fill { color, .. } if *color == theme.color.well)));
        assert!(f.list.ops.iter().any(|op| matches!(op, Op::Text { text, color, .. } if text.as_ref() == "What should we build?" && *color == theme.color.tertiary)));
        assert_eq!(f.list.hits[0].role, mypaper_ui::Role::TextInput);
        assert_eq!(action_at(&field.face(), f.list.hits[0].id), Some(Hit::Field("send".into())));
    }

    #[test]
    fn usage_legend_and_chart_share_readable_brand_inks() {
        let faces: serde_json::Value = serde_json::from_str(include_str!("../../../../../plugins/t3/golden/faces.json")).unwrap();
        let face: Face = serde_json::from_value(faces["usage"]["large"].clone()).unwrap();
        let mut ui = Ui::new().unwrap();
        let theme = Theme::new(Style::Liquid, Scheme::Dark, Rgba::rgb8([10, 132, 255], 1.0));
        let input = Interaction::default();
        let mut f = Frame::new(&mut ui.text, &theme, 1.0, &input);
        draw(&mut f, Rect::new(0.0, 0.0, 540.0, 330.0), "Usage", Icon::Terminal, Some(&face), Status::Live);
        for ink in [Rgba::rgb8([217, 119, 87], 1.0), theme.color.label] {
            assert!(f.list.ops.iter().any(|op| matches!(op, Op::Fill { rect, radius, color } if *color == ink && *radius > 0.0 && rect.width() == 7.0)), "legend dot missing: {ink:?}");
            assert!(f.list.ops.iter().any(|op| matches!(op, Op::Line { color, .. } if *color == ink)), "chart ink missing: {ink:?}");
        }
    }

    /// Weather fixtures were captured from the actual WASM component with a saved Oslo
    /// forecast and denied network permissions, using an isolated runtime data directory.
    #[test]
    fn weather_plugin_faces_render_on_the_kit() {
        use mypaper_ui::{DisplayList, TextStyle, golden, render};
        use std::{path::PathBuf, sync::Arc};
        let faces: serde_json::Value = serde_json::from_str(include_str!("../../../../../plugins/weather/golden/faces.json")).unwrap();
        let mut ui = Ui::new().unwrap();
        let theme = Theme::new(Style::Liquid, Scheme::Dark, Rgba::rgb8([10, 132, 255], 1.0));
        let input = Interaction::default();
        let mut sheet = DisplayList::default();
        sheet.ops.push(Op::Fill { rect: Rect::new(0.0, 0.0, 1176.0, 400.0), radius: 0.0, color: Rgba::rgb8([20, 25, 33], 1.0) });
        sheet.ops.push(Op::Text { rect: Rect::new(24.0, 20.0, 1100.0, 24.0), text: "weather / now — compact · regular · large".into(), style: TextStyle::new("Segoe UI Variable", 14.0), color: Rgba::rgb8([220, 228, 240], 1.0) });
        for (x, (width, height), class) in [(24.0, (180, 132), "small"), (228.0, (360, 220), "medium"), (612.0, (540, 330), "large")] {
            let face: Face = serde_json::from_value(faces[class].clone()).unwrap();
            let mut f = Frame::new(&mut ui.text, &theme, 1.0, &input);
            draw(&mut f, Rect::new(0.0, 0.0, width as f32, height as f32), "Weather", Icon::CloudSun, Some(&face), Status::Live);
            for hit in &f.list.hits {
                assert!(hit.rect.left >= 0.0 && hit.rect.top >= 0.0 && hit.rect.right <= width as f32 && hit.rect.bottom <= height as f32);
            }
            let list = f.list;
            let image = render(&mut ui, &list, width, height).unwrap();
            let rect = Rect::new(x, 50.0, width as f32, height as f32);
            sheet.ops.push(Op::Fill { rect, radius: 22.0, color: Rgba::rgb8([44, 52, 64], 1.0) });
            sheet.ops.push(Op::Image { rect, image: Arc::new(image), opacity: 1.0, source: None });
        }
        let path = std::env::var_os("MYPAPER_WEATHER_SHEET").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/weather/golden/sizes.png"));
        golden::check(&render(&mut ui, &sheet, 1176, 400).unwrap(), &path).unwrap();
    }
}
