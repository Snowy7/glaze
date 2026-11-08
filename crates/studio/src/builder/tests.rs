use super::*;

#[test]
fn contract_is_validated_by_the_engine() {
    validate_theme(r##"{"name":"Test","wallpaper":{"kind":"stack","base":{"kind":"color","color":"#202124"},"layers":[]}}"##).unwrap();
    assert!(validate_theme(r##"{"name":"Test","wallpaper":{"kind":"stack","scale":2,"base":{"kind":"color","color":"#202124"},"layers":[]}}"##).is_err());
}

fn fixture(dark: bool, ui: &mypaper_ui::Ui) -> App {
    let mut app = crate::goldens::window_app(dark, ui, (1440, 960));
    begin(&mut app, None).unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let effect_root = repo.join("effects").canonicalize().unwrap();
    state(&mut app).effects.retain(|effect| {
        effect
            .dir
            .canonicalize()
            .is_ok_and(|dir| dir.starts_with(&effect_root))
    });
    import(
        &mut app,
        &repo.join("themes/rain-city/media/city.jpg"),
        false,
    )
    .unwrap();
    let editor = active(&app).unwrap();
    let mut value = raw(&app);
    value["name"] = json!("After the rain");
    value["wallpaper"]["layers"] = json!([
        {"id":"Fine drizzle","kind":"effect","effect":"drizzle","params":{"amount":0.58},"mix":0.8,"enabled":true},
        {"id":"Film looks","kind":"effect","effect":"looks","params":{},"mix":0.6,"enabled":true},
        {"id":"Bloom","kind":"effect","effect":"bloom","params":{},"mix":0.24,"enabled":true}
    ]);
    app.docs
        .edit(
            "Fixture",
            &[crate::docs::Op {
                doc: editor.doc,
                path: String::new(),
                value: Some(value),
            }],
        )
        .unwrap();
    app
}

#[test]
fn render_builder_window_goldens() {
    use mypaper_ui::{Ui, golden};
    let out = std::env::var_os("MYPAPER_STUDIO_WINDOW_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\mypaper-targets\studio-create\shots"));
    let baseline = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/window");
    let mut ui = Ui::new().unwrap();
    for dark in [true, false] {
        let mut app = fixture(dark, &ui);
        for (name, page) in [
            ("create", Page::Create),
            ("create-effects", Page::Effects),
            ("create-params", Page::Create),
            ("create-shader-error", Page::Shader),
        ] {
            if name == "create-params" {
                state(&mut app).selected = Some(0);
            }
            if page == Page::Shader {
                state(&mut app).selected = None;
                execute(&mut app, Command::Base("shader")).unwrap();
                execute(&mut app, Command::Shader).unwrap();
                state(&mut app).source="// @param amount float 0..1 = 0.5 \"Amount\"\n\nfn wallpaper(frag: vec2f) -> vec4f {\n    let uv = frag / paper.resolution;\n    let glow = smoothstep(0.8, 0.0, length(uv - 0.5));\n    return vec4f(uv, glow * intensity, 1.0);\n}\n".into();
                validate_code(&mut app);
                assert!(!active(&app).unwrap().diagnostics.is_empty());
            }
            app.panel.as_mut().unwrap().page = page;
            let image = crate::goldens::shot(&mut app, &mut ui);
            let file = format!(
                "{name}-1440x960-{}.png",
                if dark { "dark" } else { "light" }
            );
            golden::write(&image, &out.join(&file)).unwrap();
            if std::env::var_os("MYPAPER_BLESS").is_some() {
                golden::write(&image, &baseline.join(&file)).unwrap();
            } else {
                golden::check(&image, &baseline.join(&file)).unwrap();
            }
        }
    }
}

#[test]
fn draft_edits_reorder_and_undo_without_changing_desktop() {
    let ui = mypaper_ui::Ui::new().unwrap();
    let mut app = fixture(true, &ui);
    let desktop = app.docs.doc(DocId::Config).raw.clone();
    let original = layers(&app);
    reorder(&mut app, 0, 2);
    assert_eq!(layers(&app)[2], original[0]);
    app.run(Action::Undo);
    assert_eq!(layers(&app), original);
    assert_eq!(app.docs.doc(DocId::Config).raw, desktop);
}

#[test]
fn shader_diagnostics_point_to_user_source_lines() {
    let ui = mypaper_ui::Ui::new().unwrap();
    let mut app = fixture(true, &ui);
    execute(&mut app, Command::Base("shader")).unwrap();
    execute(&mut app, Command::Shader).unwrap();
    validate_code(&mut app);
    assert!(
        active(&app).unwrap().diagnostics.is_empty(),
        "{:?}",
        active(&app).unwrap().diagnostics
    );
    state(&mut app).source =
        "fn wallpaper(frag: vec2f) -> vec4f {\n    return missing_value;\n}".into();
    validate_code(&mut app);
    assert_eq!(active(&app).unwrap().diagnostics[0].line, 2);
}

#[test]
fn save_packages_assets_credits_and_defaults_without_overwriting_unknown_themes() {
    let ui = mypaper_ui::Ui::new().unwrap();
    let mut app = fixture(true, &ui);
    let root = std::env::temp_dir().join(format!("mypaper-builder-save-{}", unique()));
    let existing = root.join("after-the-rain");
    std::fs::create_dir_all(&existing).unwrap();
    std::fs::write(existing.join("theme.json"), "newer-format-do-not-touch").unwrap();
    app.docs
        .edit(
            "Default",
            &[crate::docs::Op {
                doc: active(&app).unwrap().doc,
                path: "/wallpaper/layers/0/params/amount".into(),
                value: Some(json!(0.4)),
            }],
        )
        .unwrap();
    let preview = active(&app).unwrap().dir.join("preview.jpg");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../themes/rain-city/media/city.jpg"),
        preview,
    )
    .unwrap();
    let target = storage::publish(&app, &root).unwrap();
    assert_eq!(
        std::fs::read_to_string(existing.join("theme.json")).unwrap(),
        "newer-format-do-not-touch"
    );
    assert_eq!(target.file_name().unwrap(), "after-the-rain-2");
    let value: Value =
        serde_json::from_slice(&std::fs::read(target.join("theme.json")).unwrap()).unwrap();
    assert!(
        value["wallpaper"]["layers"][0]["params"]
            .get("amount")
            .is_none()
    );
    assert!(
        target
            .join(value["wallpaper"]["base"]["source"].as_str().unwrap())
            .is_file()
    );
    assert!(target.join("effects/drizzle/effect.json").is_file());
    assert!(target.join("preview.jpg").is_file());
    let credits: Vec<Value> =
        serde_json::from_slice(&std::fs::read(target.join("credits.json")).unwrap()).unwrap();
    assert!(credits.iter().any(|c| c["author"] == "mypaper"));
    assert!(credits.iter().any(|c| c["name"] == "city.jpg"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_versions_use_document_undo_and_custom_annotations_update_the_schema() {
    let ui = mypaper_ui::Ui::new().unwrap();
    let mut app = fixture(true, &ui);
    execute(&mut app, Command::Base("shader")).unwrap();
    execute(&mut app, Command::Shader).unwrap();
    let original = code(&app);
    let source = "fn wallpaper(frag: vec2f) -> vec4f { return vec4f(1.0); }";
    code_changed(&mut app, source.into());
    flush_code(&mut app).unwrap();
    let reference = raw(&app)["wallpaper"]["base"]["source"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        std::fs::read_to_string(active(&app).unwrap().dir.join(&reference)).unwrap(),
        source
    );
    app.run(Action::Undo);
    sync_code(&mut app).unwrap();
    assert_eq!(code(&app), original);
    app.run(Action::Redo);
    sync_code(&mut app).unwrap();
    assert_eq!(code(&app), source);
    state(&mut app).selected = Some(0);
    execute(&mut app, Command::Shader).unwrap();
    code_changed(&mut app,"// @param amount float 0..1 = 0.3 \"Amount\"\nfn effect(frag: vec2f, uv: vec2f) -> vec4f { return sample_src(uv) * params.amount; }".into());
    flush_code(&mut app).unwrap();
    validate_code(&mut app);
    assert!(
        active(&app).unwrap().diagnostics.is_empty(),
        "{:?}",
        active(&app).unwrap().diagnostics
    );
    let id = raw(&app)["wallpaper"]["layers"][0]["effect"]
        .as_str()
        .unwrap()
        .to_owned();
    let effect = active(&app)
        .unwrap()
        .effects
        .iter()
        .find(|e| e.id == id)
        .unwrap();
    assert_eq!(effect.params[0].id, "amount");
}

#[test]
fn multiline_field_changes_reach_the_shader_buffer_without_native_input() {
    use mypaper_ui::{Event, Kind, Rect, Role, Surface, TextField};
    let ui = mypaper_ui::Ui::new().unwrap();
    let mut app = fixture(true, &ui);
    execute(&mut app, Command::Base("shader")).unwrap();
    execute(&mut app, Command::Shader).unwrap();
    let surface = Surface::new_deferred_accessibility(
        Kind::App { min: (320, 240) },
        "Builder multiline test (hidden)",
        Role::Window,
    )
    .unwrap();
    let field = TextField::new(
        surface.hwnd(),
        Rect::new(0.0, 0.0, 500.0, 400.0),
        true,
        13.0,
        app.theme.palette.ink1,
    )
    .unwrap();
    let source = "fn wallpaper(frag: vec2f) -> vec4f {\n    return vec4f(1.0);\n}\n";
    field.set_text(source).unwrap();
    app.field = Some(crate::app::Field {
        id: crate::actions::FieldId::ShaderCode,
        field,
    });
    app.event(Event::TextChanged);
    assert_eq!(code(&app).replace("\r\n", "\n"), source);
    assert!(active(&app).unwrap().pending_code.is_some());
    flush_code(&mut app).unwrap();
    app.field = None;
}

#[test]
#[ignore = "explicit headless DX12 engine + own hidden-window PrintWindow check"]
fn live_engine_canvas_and_native_window() {
    use mypaper_ui::{Kind, Role, Surface, Ui, golden};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindowVisible};
    let mut ui = Ui::new().unwrap();
    let mut app = fixture(true, &ui);
    package_effects(&mut app).unwrap();
    let dir = active(&app).unwrap().dir.clone();
    let snapshot = dir.join("preview.jpg");
    let foreground = unsafe { GetForegroundWindow() };
    state(&mut app).preview.tick(&dir);
    let deadline = Instant::now() + Duration::from_secs(25);
    while !snapshot.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        snapshot.is_file(),
        "{:?}",
        active(&app).unwrap().preview.error
    );
    let out = PathBuf::from(r"C:\mypaper-targets\studio-create\live");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::copy(&snapshot, out.join("engine-snapshot.jpg")).unwrap();
    let mut surface = Surface::new_deferred_accessibility(
        Kind::App { min: (960, 640) },
        "Builder live preview (hidden)",
        Role::Window,
    )
    .unwrap();
    surface.set_no_activate(true);
    crate::panel::fit_client(&surface, (-16000, -16000, 1440, 960));
    surface.show(); // SW_SHOWNOACTIVATE, offscreen; no user window is moved or focused.
    app.size = surface.client_size();
    for dark in [true, false] {
        app.theme = crate::theme::Theme::fixed(dark, &ui.text);
        app.images.borrow_mut().clear_page();
        let built = crate::view::build(&app, &mut ui);
        surface.set_background(crate::panel::palette_for(&app).chrome);
        surface
            .present(&mut ui, (0, 0), app.size, built.list)
            .unwrap();
        let image = print_own_window(surface.hwnd(), app.size);
        golden::write(
            &image,
            &out.join(format!(
                "create-live-printwindow-{}.png",
                if dark { "dark" } else { "light" }
            )),
        )
        .unwrap();
        assert!(unsafe { IsWindowVisible(surface.hwnd()) }.as_bool());
    }
    surface.hide();
    assert!(!unsafe { IsWindowVisible(surface.hwnd()) }.as_bool());
    assert_eq!(
        unsafe { GetForegroundWindow() },
        foreground,
        "verification must not activate any window"
    );
    // A still color edit must change the real rendered pixels, then settle without a loop.
    app.docs
        .edit(
            "Still preview",
            &[crate::docs::Op {
                doc: active(&app).unwrap().doc,
                path: "/wallpaper".into(),
                value: Some(
                    json!({"kind":"stack","base":{"kind":"color","color":"#202124"},"layers":[]}),
                ),
            }],
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        app.images.borrow_mut().clear_page();
        if let Some(image) = app.images.borrow_mut().get(&snapshot, (128, 72))
            && image.pixels[0].abs_diff(36) < 8
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the engine did not reload the still base"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let stamp = std::fs::metadata(&snapshot).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        std::fs::metadata(&snapshot).unwrap().modified().unwrap(),
        stamp,
        "still preview must rest"
    );
    app.docs
        .edit(
            "Color",
            &[crate::docs::Op {
                doc: active(&app).unwrap().doc,
                path: "/wallpaper/base/color".into(),
                value: Some(json!("#d0d0d0")),
            }],
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        app.images.borrow_mut().clear_page();
        if let Some(image) = app.images.borrow_mut().get(&snapshot, (128, 72))
            && image.pixels[0] > 185
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "color edit did not reach the engine"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(unsafe { GetForegroundWindow() }, foreground);
    app.panel.as_mut().unwrap().builder.take();
    assert!(!dir.exists(), "closing the editor must remove its draft");
}

fn print_own_window(
    window: windows::Win32::Foundation::HWND,
    size: (u32, u32),
) -> mypaper_ui::Image {
    use windows::Win32::{Graphics::Gdi::*, UI::WindowsAndMessaging::GetWindowThreadProcessId};
    #[link(name = "user32")]
    unsafe extern "system" {
        fn PrintWindow(
            window: windows::Win32::Foundation::HWND,
            dc: HDC,
            flags: u32,
        ) -> windows::core::BOOL;
    }
    // SAFETY: read and paint only the hidden HWND created by this test; no desktop capture.
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        assert_eq!(pid, std::process::id());
        let dc = CreateCompatibleDC(None);
        assert!(!dc.is_invalid());
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size.0 as i32,
                biHeight: -(size.1 as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        let old = SelectObject(dc, bitmap.into());
        let painted = PrintWindow(window, dc, 3).as_bool();
        let mut pixels =
            std::slice::from_raw_parts(bits.cast::<u8>(), (size.0 * size.1 * 4) as usize).to_vec();
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        assert!(painted);
        assert!(
            pixels.chunks_exact(4).any(|pixel| pixel[0] > 100),
            "PrintWindow returned an empty frame"
        );
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        mypaper_ui::Image {
            width: size.0,
            height: size.1,
            pixels,
        }
    }
}
