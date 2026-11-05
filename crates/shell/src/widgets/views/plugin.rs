//! SDK plugin faces (docs/plugins.md, "Views"): the host draws a plugin's declarative view tree
//! with the kit, inside the standard integration chrome. Pure: no plugin I/O here. Actions are
//! numbered in tree order, so a click maps back to its action id with [`action_at`].
use mypaper_plugin_sdk::{Align as VAlign, Face, Role as VRole, Tone, View};
use mypaper_ui::chart::{LimitBar, Series, area_chart, limit_bar};
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
        View::Card { child, .. } | View::Animated { child, .. } => collect(child, out),
        View::Button { action, .. } | View::IconButton { action, .. } => out.push(Hit::Action(action.clone())),
        View::Field { action, .. } => out.push(Hit::Field(action.clone())),
        View::Select { action, options, selected, .. } => out.push(Hit::Select { action: action.clone(), options: options.clone(), selected: selected.clone() }),
        View::ListRow { action: Some(action), .. } => out.push(Hit::Action(action.clone())),
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
            f.empty(body, Some(header), "Starting…", None);
        }
    }
}

/// Whether a node takes the free space of its stack (charts, empties, spacers).
fn flexible(view: &View) -> bool {
    match view {
        View::Animated { child, .. } => flexible(child),
        _ => matches!(view, View::Spacer | View::AreaChart { .. } | View::Empty { .. } | View::List { .. }),
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
        View::IconButton { .. } => f.px(28.0),
        View::Ring { .. } => f.px(48.0),
        View::LimitBar { .. } => f.px(22.0),
        View::SegmentedBar { .. } => f.px(8.0),
        View::Sparkline { .. } => f.px(28.0),
        View::AreaChart { .. } | View::Empty { .. } | View::List { .. } => f.px(80.0),
        View::Canvas { height, .. } => f.px(height.clamp(0.0, 2048.0)),
        View::Animated { child, .. } => self::height(f, child, width),
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
            // Icons and buttons keep their natural width; the rest share what's left.
            let natural = |f: &mut Frame<'_>, c: &View| match c {
                View::Icon { size, .. } => Some(f.px(size.unwrap_or(16.0))),
                View::Button { label, icon: i, .. } => Some(f.button_size(label, i.is_some()).0),
                View::IconButton { .. } => Some(f.px(28.0)),
                View::Metric { width: Some(width), .. } => Some(f.px(*width)),
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
            f.button(id, rect, placeholder, None, ButtonKind::Gray, true);
        }
        View::ListRow { title, detail, icon: i, trailing, action } => {
            let id = if action.is_some() { take_hit() } else { 0 };
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
            limit_bar(f, rect, &bar);
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
                .map(|(s, v)| Series { color: tone(f, s.color.as_ref(), f.theme.color.accent), values: v })
                .collect();
            let unit = unit.clone().unwrap_or_default();
            let labels = [x_labels[0].as_str(), x_labels[1].as_str(), x_labels[2].as_str()];
            area_chart(f, rect, &kit, labels, |v| format!("{}{unit}", compact_number(v)));
        }
        View::Empty { icon: i, title, detail } => {
            f.empty(rect, i.as_deref().map(icon), title, detail.as_deref());
        }
        View::Canvas { commands, .. } => canvas::draw(f, rect, commands, |f, t| tone(f, Some(t), f.theme.color.label)),
        View::Animated { stiffness, damping, child } => animated::draw(f, rect, child, (*stiffness, *damping), next),
        View::Spacer | View::Unknown => {}
    }
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
