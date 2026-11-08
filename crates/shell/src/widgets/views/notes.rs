//! Notes: the old Markdown subset drawn as text (headings, bullets, tasks, quotes, rules,
//! **bold**, *italic*, `code`, ~~strike~~), scrolling when long. A click opens the editor (a
//! RichEdit field over the widget); a task's box toggles it.
use super::{Ctx, FIELD, NOTE_TASK, field};
use crate::widgets::markdown::{self, Emphasis};
use glaze_ui::icons::Icon;
use glaze_ui::{Op, Rect, Role, TextStyle};

/// A laid-out block: its source line, top and height, and what it is.
struct Block {
    line: usize,
    top: f32,
    height: f32,
    task: Option<bool>,
    quote: bool,
    rule: bool,
    bullet: bool,
}

pub fn draw(c: &mut Ctx, r: Rect) {
    if c.m.field.is_some() {
        return field(c, r, "Note", None);
    }
    c.hit(FIELD, r, Role::Button, "Edit note");
    if c.m.note.trim().is_empty() {
        return c.empty(r, Some(Icon::Edit), "Click to write", Some("# a heading, - [ ] a task"));
    }
    let color = c.color();
    let s = c.s();
    let base = (c.rect.width().min(c.rect.height()) / s * 0.034).clamp(11.5, 15.0)
        * c.m.num("fontScale", 1.0) as f32;
    let width = r.width();
    // Lay out every block first, so the scroll view knows the content's height.
    // x, y, text, style, emphasis, muted, struck (a done task)
    let mut runs: Vec<(f32, f32, String, TextStyle, Emphasis, bool, bool)> = Vec::new();
    let mut blocks = Vec::new();
    let mut y = 0.0;
    let note = c.m.note.clone();
    for (index, raw) in note.lines().enumerate() {
        let line = raw.trim_end();
        if line.is_empty() {
            y += (base * 0.6 * s).round();
            continue;
        }
        let top = y;
        let rule = line.len() >= 3 && (line.chars().all(|ch| ch == '-') || line.chars().all(|ch| ch == '*'));
        if rule {
            let h = (base * s).round();
            blocks.push(Block { line: index, top, height: h, task: None, quote: false, rule: true, bullet: false });
            y += h;
            continue;
        }
        let level = line.chars().take_while(|&ch| ch == '#').count();
        let heading = if (1..=3).contains(&level) && line.as_bytes().get(level) == Some(&b' ') { level } else { 0 };
        let task = markdown::task_box(line);
        let bullet = task.is_none() && (line.trim_start().starts_with("- ") || line.trim_start().starts_with("* "));
        let quote = line.starts_with('>');
        let value = if heading > 0 {
            line[heading + 1..].to_owned()
        } else if let Some(at) = task {
            line[at + 2..].trim_start().to_owned()
        } else if bullet {
            line.trim_start()[2..].trim_start().to_owned()
        } else if quote {
            line[1..].trim_start().to_owned()
        } else {
            line.to_owned()
        };
        let checked = task.map(|at| line.as_bytes()[at] != b' ');
        let size = base * match heading {
            1 => 1.35,
            2 => 1.15,
            _ => 1.0,
        };
        let inset = if task.is_some() || bullet {
            (base * 1.6 * s).round()
        } else if quote {
            (base * 0.9 * s).round()
        } else {
            0.0
        };
        let line_h = (size * 1.45 * s).round();
        let mut x = inset;
        let value = if heading == 3 { value.to_uppercase() } else { value };
        for span in markdown::inline(&value) {
            let mut style = c.display(size, if heading > 0 || span.emphasis == Emphasis::Bold { 650 } else { 400 });
            if heading == 3 {
                style.tracking = 0.06 * size * s;
            }
            if span.emphasis == Emphasis::Code {
                style.family = "Cascadia Mono".into();
                style.size = (style.size * 0.9).round();
            }
            let muted = heading == 3 || checked == Some(true);
            for word in span.text.split_inclusive(char::is_whitespace) {
                let w = c.f.text.measure(word, &style).0;
                if x > inset && x + w > width {
                    y += line_h;
                    x = inset;
                }
                if w > width - inset {
                    for ch in word.chars() {
                        let t = ch.to_string();
                        let cw = c.f.text.measure(&t, &style).0;
                        if x > inset && x + cw > width {
                            y += line_h;
                            x = inset;
                        }
                        runs.push((x, y, t, style.clone(), span.emphasis, muted, checked == Some(true)));
                        x += cw;
                    }
                } else {
                    runs.push((x, y, word.to_owned(), style.clone(), span.emphasis, muted, checked == Some(true)));
                    x += w;
                }
            }
        }
        y += line_h + (base * 0.2 * s).round();
        blocks.push(Block { line: index, top, height: y - top, task: checked, quote, rule: false, bullet });
    }
    let line_h = (base * 1.45 * s).round();
    c.scroll(r, y, |c, top| {
        for b in &blocks {
            let y = top + b.top;
            if b.rule {
                c.f.fill(Rect::new(r.left, (y + b.height / 2.0).round(), r.width(), 1.0_f32.max(s.floor())), 0.0, color.separator);
            }
            if b.quote {
                c.f.fill(Rect::new(r.left, y, c.px(2.0), b.height - (base * 0.2 * s).round()), c.px(1.0), color.tertiary);
            }
            if b.bullet {
                let d = (base * 0.34 * s).round();
                c.f.fill(Rect::new(r.left + (base * 0.4 * s).round(), (y + (line_h - d) / 2.0).round(), d, d), d / 2.0, color.secondary);
            }
            if let Some(checked) = b.task {
                let d = (base * 1.05 * s).round();
                let at = Rect::new(r.left, (y + (line_h - d) / 2.0).round(), d, d);
                let id = NOTE_TASK + b.line as u32;
                let radius = (d * 0.3).round();
                c.f.fill(at, radius, if checked { color.accent } else { color.fill });
                c.f.wash(id, at, radius, true);
                c.f.focus_ring(id, at, radius);
                if checked {
                    c.icon(at, Icon::Check, (d / s * 0.72).round(), color.on_accent);
                }
                let target = Rect::new(r.left, y, (base * 1.6 * s).round(), b.height);
                c.f.hit(id, target, Role::CheckBox, "Toggle note task", true, Some(checked));
            }
        }
        for (x, y, text, style, emphasis, muted, struck) in &runs {
            let (w, h) = c.f.text.measure(text, style);
            let rect = Rect::new(r.left + x, top + y + ((line_h.max(style.size * 1.45) - h) / 2.0).round(), w, h);
            if rect.bottom < r.top || rect.top > r.bottom {
                continue;
            }
            if *emphasis == Emphasis::Code {
                c.f.fill(rect.inflate(c.px(1.0)), c.px(4.0), color.well);
            }
            c.f.push(Op::Emphasis {
                rect,
                text: text.as_str().into(),
                style: style.clone(),
                color: if *muted { color.secondary } else { color.label },
                italic: *emphasis == Emphasis::Italic,
                strike: *struck || *emphasis == Emphasis::Strike,
            });
        }
    });
}
