//! To do: the title and what is left, a track of what is done, the tasks (a round check, the
//! text, a delete button on hover) and the add field. Short widgets keep only the open tasks.
use super::{Ctx, DELETE_TASK, EDIT_TASK, ITEM, field};
use mypaper_ui::icons::Icon;
use mypaper_ui::{Align, Op, Rect, Rgba, Role, TextRole};

pub fn draw(c: &mut Ctx, r: Rect) {
    let color = c.color();
    let s = c.s();
    let m = c.m;
    let (w, h) = (c.rect.width() / s, c.rect.height() / s);
    let compact = h < 124.0;
    let narrow = w < 150.0;
    let done = m.tasks.iter().filter(|t| t.done).count();
    let open = m.tasks.len() - done;
    let mut r = r;
    if !narrow && !compact {
        let (head, rest) = r.take_top(c.f.line(TextRole::Headline), c.px(6.0));
        let count = if m.tasks.is_empty() {
            String::new()
        } else if open == 0 {
            "All done".to_owned()
        } else {
            format!("{open} left")
        };
        let bw = if count.is_empty() { 0.0 } else { c.measure(&count, TextRole::Caption).0 + c.px(12.0) };
        let (badge, title) = head.take_right(bw, c.px(8.0));
        c.text(title, &m.str("title", "To do"), TextRole::Headline, color.label, Align::Start);
        if !count.is_empty() {
            let ink = if open == 0 { Some(color.accent) } else { None };
            c.f.badge(badge.left, badge.center().y, &count, ink);
        }
        r = rest;
        if !m.tasks.is_empty() {
            let (bar, rest) = r.take_top(c.px(4.0), c.px(8.0));
            c.f.progress(bar, done as f32 / m.tasks.len() as f32, Some(color.accent));
            r = rest;
        }
    }
    let control = c.px(c.f.theme.metrics.control);
    let add_top = m.str("addPosition", "bottom") == "top";
    let show_field = !compact && m.task_edit.is_none();
    let list = if show_field {
        let (bar, rest) = if add_top {
            r.take_top(control, c.px(8.0))
        } else {
            r.take_bottom(control, c.px(8.0))
        };
        field(c, bar, "Add a task", Some(Icon::Add));
        rest
    } else {
        r
    };
    let hide_done = m.flag("hideDone", false) || compact;
    let shown: Vec<_> = m.tasks.iter().enumerate().filter(|(_, t)| !hide_done || !t.done).collect();
    if shown.is_empty() {
        let (title, detail) = if m.tasks.is_empty() { ("Nothing to do", None) } else { ("All done", Some("Nice work.")) };
        return c.empty(list, Some(Icon::Check), title, detail);
    }
    let row_h = c.px(if compact { 28.0 } else { 34.0 });
    c.scroll(list, shown.len() as f32 * row_h, |c, top| {
        for (n, (i, task)) in shown.iter().enumerate() {
            let row = Rect::new(list.left, top + n as f32 * row_h, list.width(), row_h);
            task_row(c, row, *i, &task.text, task.done);
        }
    });
}

fn task_row(c: &mut Ctx, row: Rect, i: usize, text: &str, done: bool) {
    let color = c.color();
    let check_id = ITEM + i as u32;
    let edit_id = EDIT_TASK + i as u32;
    let delete_id = DELETE_TASK + i as u32;
    let d = c.px(18.0);
    let (check_box, rest) = row.take_left(d + c.px(4.0), c.px(6.0));
    let check = check_box.place(d, d, Align::Center, Align::Center);
    // The check: a quiet ring of fill when open, the accent with a tick when done.
    if done {
        c.f.fill(check, d / 2.0, color.accent);
        c.icon(check, Icon::Check, 12.0, color.on_accent);
    } else {
        c.f.fill(check, d / 2.0, color.fill);
    }
    c.f.wash(check_id, check, d / 2.0, true);
    c.f.focus_ring(check_id, check, d / 2.0);
    c.f.hit(check_id, check_box, Role::CheckBox, text, true, Some(done));
    // Narrow widgets give the whole row to the text (delete is a hover control anyway).
    let del = row.height() - c.px(8.0);
    let deletable = rest.width() >= c.px(110.0);
    let (del_box, text_box) = if deletable { rest.take_right(del, c.px(4.0)) } else { (Rect::default(), rest) };
    if c.m.task_edit == Some(i) {
        let control = c.px(c.f.theme.metrics.control_small);
        field(c, text_box.place(text_box.width(), control, Align::Start, Align::Center), "Edit task", None);
        return;
    }
    let radius = c.px(6.0);
    c.control(edit_id, text_box, radius, Role::Button, &format!("Edit {text}"), None);
    let style = c.f.font(TextRole::Body);
    let fitted = c.f.text.fit(text, &style, text_box.width() - c.px(4.0));
    let (tw, th) = c.f.text.measure(&fitted, &style);
    c.f.push(Op::Emphasis {
        rect: Rect::new(text_box.left + c.px(2.0), (text_box.center().y - th / 2.0).round(), tw, th),
        text: fitted,
        style,
        color: if done { color.secondary } else { color.label },
        italic: false,
        strike: done,
    });
    let hovered = matches!(c.m.hover, Some(h) if h == edit_id || h == delete_id || h == check_id);
    if !deletable {
        return;
    }
    let at = del_box.place(del, del, Align::Center, Align::Center);
    c.control(delete_id, at, del / 2.0, Role::Button, "Delete task", None);
    c.icon(at, Icon::Close, 12.0, if hovered { color.secondary } else { Rgba::TRANSPARENT });
}
