//! A photo frame: the picture filling the widget (**bleed**), on a light mount (**matte**), or
//! as a **polaroid** with its caption. A click shows the next photo; a change crossfades.
use super::{Ctx, PRIMARY};
use mypaper_ui::icons::Icon;
use mypaper_ui::{Align, Op, Rect, Role, Scheme, Style, TextRole, Theme};
use std::sync::Arc;

pub fn draw(c: &mut Ctx, r: Rect) {
    let variant = c.m.str("variant", "bleed");
    c.hit(PRIMARY, r, Role::Button, "Next photo");
    let Some(image) = c.m.photo.current.clone() else {
        let detail = c.m.error.clone().unwrap_or_else(|| "Choose images in Studio".into());
        return c.empty(r, Some(Icon::Photo), "Your photos", Some(&detail));
    };
    // The paper of a mount or a print is light whatever the scheme.
    let paper = Theme::new(Style::Liquid, Scheme::Light, c.color().accent).color;
    let contain = c.m.str("fit", "cover") == "contain";
    let area = match variant.as_str() {
        "matte" => {
            c.f.fill(r, c.px(5.0), paper.knob.with_alpha(0.94));
            r.inset(mypaper_ui::Insets::all((r.width().min(r.height()) * 0.065).clamp(c.px(6.0), c.px(20.0)).round()))
        }
        "polaroid" => {
            c.f.fill(r, c.px(4.0), paper.knob.with_alpha(0.96));
            let band = (r.width().min(r.height()) * 0.17).clamp(c.px(24.0), c.px(54.0)).round();
            let side = c.px(8.0);
            let photo = Rect::new(r.left + side, r.top + side, r.width() - 2.0 * side, r.height() - side - band);
            let caption = c.m.str("caption", "");
            if !caption.is_empty() {
                let at = Rect::new(r.left + side, photo.bottom, r.width() - 2.0 * side, band);
                c.text(at, &caption, TextRole::Callout, paper.label, Align::Center);
            }
            photo
        }
        _ => r,
    };
    let fraction = c.m.photo.fade.map_or(1.0, |start| {
        (c.instant.saturating_duration_since(start).as_secs_f32() / 0.35).clamp(0.0, 1.0)
    });
    if fraction < 1.0
        && let Some(previous) = c.m.photo.previous.clone()
    {
        picture(c, area, &previous, 1.0, contain);
    }
    picture(c, area, &image, fraction, contain);
}

/// `image` in `rect`: covering it (a centred crop, never stretched) or contained in it.
fn picture(c: &mut Ctx, rect: Rect, image: &Arc<mypaper_ui::Image>, opacity: f32, contain: bool) {
    let ratio = if contain {
        (rect.width() / image.width as f32).min(rect.height() / image.height as f32)
    } else {
        (rect.width() / image.width as f32).max(rect.height() / image.height as f32)
    };
    let (rect, source) = if contain {
        let (w, h) = (image.width as f32 * ratio, image.height as f32 * ratio);
        (rect.place(w, h, Align::Center, Align::Center), None)
    } else {
        let (w, h) = (rect.width() / ratio, rect.height() / ratio);
        (
            rect,
            Some(Rect::new((image.width as f32 - w) / 2.0, (image.height as f32 - h) / 2.0, w, h)),
        )
    };
    c.f.push(Op::Image {
        rect,
        image: image.clone(),
        opacity,
        source,
    });
}
