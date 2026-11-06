use super::{Command, controls::text, specimens};
use crate::{
    actions::{Action, Edit, FieldId},
    app::App,
    ui::{B, Col},
};
use mypaper_ui::controls::{Align, key};
use mypaper_ui::{Rect, Rgba};
use serde_json::{Value, json};

pub fn swatches(
    app: &App,
    b: &mut B,
    c: &mut Col,
    edit: &Edit,
    current: Option<&Value>,
    auto: bool,
    _none: bool,
    presets: &[[u8; 3]],
) {
    use mypaper_ui::color_row::{Choice, draw};
    let current = current.and_then(Value::as_str).unwrap_or("");
    let is_windows = edit.path.starts_with("/look/windows/");
    let wallpaper = specimens::wallpaper_color(app);
    let wallpaper_hex = wallpaper.map(|color| {
        mypaper_config::Color([
            (color.r * 255.0).round() as u8,
            (color.g * 255.0).round() as u8,
            (color.b * 255.0).round() as u8,
        ])
        .hex()
    });
    let linked = current == "auto"
        || !auto
            && wallpaper_hex
                .as_deref()
                .is_some_and(|hex| current.eq_ignore_ascii_case(hex));
    let as_is = if is_windows {
        let field = edit.path.rsplit('/').next().unwrap_or("");
        Action::Set(
            Edit::new(
                crate::docs::DocId::Studio,
                format!("/panel/windows/{field}"),
                edit.label.clone(),
            ),
            Value::Null,
        )
    } else {
        Action::Unset(edit.clone())
    };
    let mut choices = vec![
        Choice {
            label: "As is".into(),
            color: None,
            selected: current.is_empty(),
            action: as_is,
        },
        Choice {
            label: "From wallpaper".into(),
            color: wallpaper,
            selected: linked,
            action: if auto {
                Action::Set(edit.clone(), json!("auto"))
            } else {
                wallpaper_hex
                    .as_ref()
                    .map_or(Action::None, |hex| Action::Set(edit.clone(), json!(hex)))
            },
        },
    ];
    choices.extend(presets.iter().map(|rgb| {
        let hex = mypaper_config::Color(*rgb).hex();
        Choice {
            label: format!("{} {hex}", edit.label),
            color: Some(Rgba::rgb8(*rgb, 1.0)),
            selected: !linked && current.eq_ignore_ascii_case(&hex),
            action: Action::Set(edit.clone(), json!(hex)),
        }
    }));
    choices.push(Choice {
        label: format!("Custom {}", edit.label),
        color: mypaper_config::Color::parse(current).map(|color| Rgba::rgb8(color.0, 1.0)),
        selected: !linked
            && current.starts_with('#')
            && !presets.iter().any(|rgb| {
                mypaper_config::Color(*rgb)
                    .hex()
                    .eq_ignore_ascii_case(current)
            }),
        action: Action::More(key(("panel-color-picker", edit.doc, edit.path.as_ref()))),
    });
    draw(
        b,
        key(("panel-colors", edit.doc, edit.path.as_ref())),
        Rect::new(c.x, c.y, c.w, b.px(34.0)),
        &choices,
    );
    c.y += b.px(40.0);
    if app.is_open(key(("panel-color-picker", edit.doc, edit.path.as_ref()))) {
        let r = Rect::new(c.x, c.y, c.w, b.px(34.0));
        b.fill(r.inflate(b.px(4.0)), b.px(7.0), b.p.fill);
        text(
            b,
            Rect::new(r.left, r.top, b.px(50.0), r.height()),
            "Hex",
            12.0,
            400,
            b.p.ink2,
            Align::Start,
        );
        let field = Rect::new(
            r.left + b.px(50.0),
            r.top,
            r.width() - b.px(92.0),
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
        super::controls::icon_button(
            b,
            Rect::new(r.right - b.px(34.0), r.top, b.px(34.0), r.height()),
            mypaper_ui::icons::Icon::Palette.into(),
            "Choose a color",
            true,
            Action::Panel(Command::PickColor(edit.clone())),
        );
        c.y = r.bottom + b.px(10.0);
    }
}

pub fn pick(app: &mut App, edit: &Edit) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};
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
    // RGB movement previews the desktop; the existing set below saves only after OK.
    if crate::panel::pick_client::choose_color(app, edit, &mut picker) {
        let color = picker.rgbResult.0;
        app.set(
            edit,
            json!(
                mypaper_config::Color([color as u8, (color >> 8) as u8, (color >> 16) as u8]).hex()
            ),
        );
    }
}
