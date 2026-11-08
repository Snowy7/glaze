//! The focus timer: a ring that empties as time runs, the time left inside it, the session
//! dots, and reset, start/pause and skip. The mode switch (Focus, Short, Long, Custom) shows
//! when there is room. By the box: a **tile** (the ring with play inside), a **row** (ring,
//! then the controls beside it), or a **stack** (switch, ring, controls).
use super::{Ctx, MODE, NEXT, PRIMARY, RESET};
use glaze_ui::icons::Icon;
use glaze_ui::{Align, Point, Rect, Role, TextRole};

const MODES: [(&str, &str); 4] = [("focus", "Focus"), ("short", "Short"), ("long", "Long"), ("custom", "Custom")];

pub fn draw(c: &mut Ctx, r: Rect) {
    let s = c.s();
    let (w, h) = (c.rect.width() / s, c.rect.height() / s);
    let button = (w.min(h) * 0.15).clamp(26.0, 40.0).round();
    let play = (button * 1.3).round();
    let wide = h < 130.0 || w / h > 1.4;
    // A row needs its ring, a gap and the three controls side by side; else the tile.
    let row_fits = w - h.min(w * 0.5) - 14.0 >= button * 2.0 + play + 16.0;
    let tile = w < 130.0 || (h < 130.0 && w < 150.0) || (wide && !row_fits);
    let row = !tile && wide;
    let switch = !tile && h >= 130.0 && w >= 290.0;
    let r = if switch {
        let (bar, rest) = r.take_top(c.px(c.f.theme.metrics.control_small), c.px(10.0));
        let selected = MODES.iter().position(|(m, _)| *m == c.m.timer.mode).unwrap_or(0);
        let labels: Vec<&str> = MODES.iter().map(|(_, l)| *l).collect();
        c.f.segmented(MODE, bar, &labels, selected);
        rest
    } else {
        r
    };
    if tile {
        let d = r.width().min(r.height()).floor();
        let center = r.center();
        dial(c, center, d / 2.0, false);
        let p = c.px(play.min(d / s * 0.42));
        let at = Rect::new(center.x - p / 2.0, center.y - p / 2.0, p, p);
        play_button(c, at);
        return;
    }
    let controls_w = c.px(button) * 2.0 + c.px(play) + c.px(16.0);
    if row {
        let (ring_box, rest) = r.take_left(r.height().min(r.width() * 0.5), c.px(14.0));
        let d = ring_box.width().min(ring_box.height()).floor();
        dial(c, ring_box.center(), d / 2.0, true);
        let controls = rest.place(controls_w.min(rest.width()), c.px(play), Align::Center, Align::Center);
        buttons(c, controls, button, play);
        return;
    }
    let (controls, ring_box) = r.take_bottom(c.px(play), c.px(10.0));
    let d = ring_box.width().min(ring_box.height()).floor();
    dial(c, ring_box.center(), d / 2.0, true);
    buttons(c, controls.place(controls_w, c.px(play), Align::Center, Align::Center), button, play);
}

/// The ring, the time left inside it, and under it the mode or the session dots.
fn dial(c: &mut Ctx, center: Point, radius: f32, readout: bool) {
    let color = c.color();
    let s = c.s();
    let now = c.now.ms;
    let timer = &c.m.timer;
    let left = timer.left(now);
    let stroke = (radius / s * 0.06).clamp(3.0, 6.0).round() * s;
    let done = 1.0 - left as f32 / timer.total.max(1) as f32;
    c.f.ring(center, radius - stroke / 2.0, stroke, done, Some(color.accent));
    if !readout {
        return;
    }
    let seconds = (left + 999) / 1000;
    let value = if seconds >= 3600 {
        format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    };
    let size = (radius * 2.0 / s * if seconds >= 3600 { 0.2 } else { 0.27 }).clamp(14.0, 56.0);
    let style = c.display(size, 500);
    let running = timer.ends_at.is_some();
    let finished = left == 0;
    let (vw, vh) = c.f.text.measure(&value, &style);
    let sub_h = c.f.line(TextRole::Caption);
    let y = center.y - (vh + sub_h) / 2.0;
    let ink = if running || finished { color.label } else { color.label.with_alpha(0.7) };
    c.styled(Rect::new(center.x - vw / 2.0, y, vw + 1.0, vh), &value, &style, ink, Align::Start, Align::Center);
    let label = if finished {
        "Done".to_owned()
    } else {
        MODES
            .iter()
            .find(|(m, _)| *m == timer.mode)
            .map_or("Focus", |(_, l)| l)
            .to_uppercase()
    };
    let inner = radius * 1.3;
    let at = Rect::new(center.x - inner / 2.0, y + vh, inner, sub_h);
    if timer.mode == "focus" && !finished && radius > c.px(48.0) {
        // One dot per session before the long break: done ones filled, the current ringed.
        let total = c.m.num("sessionsBeforeLong", 4.0).clamp(1.0, 8.0) as u32;
        let d = c.px(6.0);
        let gap = c.px(5.0);
        let width = total as f32 * d + (total - 1) as f32 * gap;
        let x0 = (center.x - width / 2.0).round();
        let cy = (at.center().y - d / 2.0).round();
        for i in 0..total {
            let dot = Rect::new(x0 + i as f32 * (d + gap), cy, d, d);
            let ink = if i < timer.sessions % total {
                color.label
            } else if i == timer.sessions % total {
                color.secondary
            } else {
                color.fill
            };
            c.f.fill(dot, d / 2.0, ink);
        }
    } else {
        c.text(at, &label, TextRole::Caption, if finished { color.accent } else { color.secondary }, Align::Center);
    }
}

fn play_button(c: &mut Ctx, at: Rect) {
    let running = c.m.timer.ends_at.is_some();
    let color = c.color();
    let d = at.width();
    c.f.fill(at, d / 2.0, color.accent);
    c.f.wash(PRIMARY, at, d / 2.0, true);
    c.f.focus_ring(PRIMARY, at, d / 2.0);
    c.icon(at, if running { Icon::Pause } else { Icon::Play }, (d / c.s() * 0.42).round(), color.on_accent);
    c.f.hit(PRIMARY, at, Role::Button, "Start or pause timer", true, None);
}

/// Reset, start/pause (the filled disc) and skip, as the media transport.
fn buttons(c: &mut Ctx, r: Rect, button: f32, play: f32) {
    let s = c.s();
    let (b, p) = (c.px(button), c.px(play));
    let gap = ((r.width() - 2.0 * b - p) / 2.0).max(c.px(4.0));
    let cy = r.center().y;
    let reset = Rect::new(r.left, (cy - b / 2.0).round(), b, b);
    c.f.icon_button(RESET, reset, Icon::Refresh, b / s, "Reset timer", true);
    let disc = Rect::new(reset.right + gap, (cy - p / 2.0).round(), p, p);
    play_button(c, disc);
    let skip = Rect::new(disc.right + gap, reset.top, b, b);
    c.f.icon_button(NEXT, skip, Icon::Next, b / s, "Skip to next session", true);
}
