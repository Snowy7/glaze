use super::{
    Command,
    controls::{button, text},
    specimens,
};
use crate::{
    actions::{Action, Edit, FieldId},
    app::App,
    ui::{B, Col},
};
use mypaper_ui::controls::{Align, Behavior, key};
use mypaper_ui::{Rect, Rgba, Role};
use serde_json::{Value, json};

pub fn swatches(
    app: &App,
    b: &mut B,
    c: &mut Col,
    edit: &Edit,
    current: Option<&Value>,
    auto: bool,
    none: bool,
    presets: &[[u8; 3]],
) {
    let no_tint = none
        && app
            .get(crate::docs::DocId::Config, "/look/material/clarity")
            .and_then(Value::as_f64)
            == Some(1.0)
        && app
            .get(crate::docs::DocId::Config, "/look/material/tint")
            .is_none();
    let current = if no_tint {
        ""
    } else {
        current
            .and_then(Value::as_str)
            .unwrap_or(if auto { "auto" } else { "" })
    };
    if none {
        let r = Rect::new(c.x, c.y + b.px(2.0), b.px(70.0), b.px(28.0));
        button(
            b,
            r,
            if no_tint { "✓ None" } else { "None" },
            None,
            false,
            true,
            Action::Panel(Command::NoTint),
        );
        c.y = r.bottom + b.px(8.0);
    }
    let mut x = c.x;
    let y = c.y;
    let d = b.px(26.0);
    if auto {
        let r = Rect::new(x, y, b.px(142.0).min(c.w), b.px(30.0));
        button(
            b,
            r,
            "From wallpaper",
            None,
            false,
            true,
            Action::Set(edit.clone(), json!("auto")),
        );
        let chip = Rect::new(
            r.left + b.px(5.0),
            r.top + b.px(5.0),
            b.px(20.0),
            b.px(20.0),
        );
        b.fill(
            chip,
            b.px(5.0),
            specimens::wallpaper_color(app).unwrap_or(b.p.fill),
        );
        for offset in [0.0, 5.0] {
            b.stroke(
                Rect::new(
                    chip.left + b.px(4.0 + offset),
                    chip.top + b.px(7.0),
                    b.px(7.0),
                    b.px(5.0),
                ),
                b.px(2.0),
                b.px(1.0),
                b.p.ink1,
            );
        }
        if current == "auto" {
            b.stroke(r, b.px(7.0), b.px(2.0), b.p.ink1);
        }
        // Automatic is always a labeled linked chip, distinct from every preset dot.
        x = r.right + b.px(12.0);
    }
    for color in presets {
        if x + d > c.right() {
            x = c.x;
            c.y += b.px(36.0);
        }
        let r = Rect::new(x, c.y, d, d);
        let hex = mypaper_config::Color(*color).hex();
        let color = Rgba::rgb8(*color, 1.0);
        b.fill(r, d / 2.0, color);
        b.stroke(r, d / 2.0, b.px(1.0), b.p.edge);
        let on = current.eq_ignore_ascii_case(&hex);
        if on {
            b.icon(
                r,
                crate::theme::glyph::CHECK,
                15.0,
                if mypaper_ui::theme::luminance(color) > 0.4 {
                    super::tokens::hex(0x111111)
                } else {
                    super::tokens::hex(0xffffff)
                },
            );
        }
        let k = key(("panel-color", &edit.path, &hex));
        b.hit_checked(
            k,
            r,
            Role::RadioButton,
            &format!("{} {hex}", edit.label),
            Some(on),
            Action::Set(edit.clone(), json!(hex)),
            Behavior::Click,
        );
        b.focus_ring(k, r, d / 2.0);
        x += d + b.px(9.0);
    }
    c.y += b.px(38.0);
    let r = Rect::new(c.x, c.y, c.w, b.px(32.0));
    text(
        b,
        Rect::new(r.left, r.top, b.px(62.0), r.height()),
        "Custom",
        12.0,
        400,
        b.p.ink2,
        Align::Start,
    );
    let field = Rect::new(
        r.left + b.px(66.0),
        r.top,
        (r.width() - b.px(106.0)).max(1.0),
        r.height(),
    );
    crate::ui::text_field(
        app,
        b,
        field,
        FieldId::Text(edit.doc, edit.path.clone()),
        None,
        "#RRGGBB",
        if current.starts_with('#') {
            current
        } else {
            ""
        },
        &format!("{} hex", edit.label),
    );
    b.stroke(field, b.px(6.0), b.px(1.0), b.p.edge);
    let picker = Rect::new(r.right - b.px(32.0), r.top, b.px(32.0), b.px(32.0));
    super::controls::icon_button(
        b,
        picker,
        mypaper_ui::icons::Icon::Palette.into(),
        &format!("Pick {} color", edit.label),
        true,
        Action::Panel(Command::PickColor(edit.clone())),
    );
    c.y = r.bottom + b.px(8.0);
}

pub fn pick(app: &mut App, edit: &Edit) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::Controls::Dialogs::{
        CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW,
    };
    if app.fixture {
        return;
    }
    let [r, g, b] = app
        .get(edit.doc, &edit.path)
        .and_then(Value::as_str)
        .and_then(mypaper_config::Color::parse)
        .map_or([128, 128, 128], |c| c.0);
    let mut custom = [COLORREF(0); 16];
    let mut picker = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: app.window.unwrap_or_default(),
        rgbResult: COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16)),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    // SAFETY: native modal dialog with our own owner and a live 16-color buffer.
    if unsafe { ChooseColorW(&mut picker) }.as_bool() {
        let color = picker.rgbResult.0;
        app.set(
            edit,
            json!(
                mypaper_config::Color([color as u8, (color >> 8) as u8, (color >> 16) as u8]).hex()
            ),
        );
    }
}
