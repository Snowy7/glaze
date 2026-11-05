//! Opt-in v16 Studio panel. It shares the legacy app's documents, actions, input and workers;
//! only window placement, navigation and presentation belong to this module.

mod controls;
mod pages;
mod tokens;

#[cfg(test)]
mod tests;

use mypaper_ui::controls::{Builder, key};
use mypaper_ui::{Chrome, Rect, Text};

use crate::actions::{Action, FieldId};
use crate::app::App;
use crate::view::Built;

pub const TITLE: &str = "mypaper Studio · Panel";

pub fn enabled() -> bool {
    std::env::var("MYPAPER_STUDIO_PANEL").is_ok_and(|v| v == "1")
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Page {
    #[default]
    Root,
    Wallpapers,
    Look,
    Bars,
    Top,
    Dock,
    Widgets,
    DesktopMenu,
    Windows,
    Connections,
    Settings,
    Performance,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Self::Root => "Studio",
            Self::Wallpapers => "Wallpaper",
            Self::Look => "Look",
            Self::Bars => "Bars",
            Self::Top => "Top bar",
            Self::Dock => "Dock",
            Self::Widgets => "Widgets",
            Self::DesktopMenu => "Desktop menu",
            Self::Windows => "Windows",
            Self::Connections => "Connections",
            Self::Settings => "Settings",
            Self::Performance => "Performance and power",
        }
    }

    fn parent(self) -> Option<Self> {
        match self {
            Self::Root => None,
            Self::Performance => Some(Self::Settings),
            Self::Top | Self::Dock => Some(Self::Bars),
            _ => Some(Self::Root),
        }
    }
}

#[derive(Debug, Default)]
pub struct State {
    pub page: Page,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Open(Page),
    Back,
    Done,
}

pub fn run(app: &mut App, command: Command) {
    app.blur_field();
    let Some(state) = app.panel.as_mut() else {
        return;
    };
    match command {
        Command::Open(page) => state.page = page,
        Command::Back => state.page = state.page.parent().unwrap_or(Page::Root),
        Command::Done => app.quit = true,
    }
    app.input.focus = None;
    app.input.hover = None;
    app.dirty = true;
}

/// Panel shortcuts are resolved before the legacy app's page shortcuts.
pub fn on_key(app: &mut App, vk: u16, (ctrl, shift, _alt): (bool, bool, bool)) -> bool {
    if !ctrl {
        return false;
    }
    match vk {
        0x5A if !shift => app.run(Action::Undo),
        0x5A | 0x59 => app.run(Action::Redo),
        0x57 => run(app, Command::Done),
        0x4B | 0x46 => {
            run(app, Command::Open(Page::Root));
            app.focus_field(FieldId::WallpaperSearch);
        }
        0x31..=0x37 => run(
            app,
            Command::Open(
                [
                    Page::Wallpapers,
                    Page::Look,
                    Page::Bars,
                    Page::Widgets,
                    Page::DesktopMenu,
                    Page::Windows,
                    Page::Connections,
                ][usize::from(vk - 0x31)],
            ),
        ),
        0xBC => run(app, Command::Open(Page::Settings)),
        // History, Show desktop and Peek are present but deferred in this increment.
        0x48 | 0x44 => {}
        _ => return false,
    }
    true
}

pub fn palette_for(app: &App) -> mypaper_ui::controls::Palette {
    if app.panel.is_some() && !app.theme.contrast {
        tokens::palette(app.theme.palette.dark)
    } else {
        app.theme.palette
    }
}

pub fn build(app: &App, text: &mut Text) -> Built {
    let page = app.panel.as_ref().map_or(Page::Root, |s| s.page);
    let p = palette_for(app);
    let ty = app.theme.ty.clone();
    let mut b = Builder::new(text, &p, &ty, app.scale, &app.input);
    let window = Rect::new(0.0, 0.0, app.size.0 as f32, app.size.1 as f32);
    b.fill(window, 0.0, p.chrome);
    let header = Rect::new(0.0, 0.0, window.width(), b.px(48.0));
    pages::header(app, &mut b, header, page);
    let body = Rect::new(
        0.0,
        header.bottom,
        window.width(),
        window.height() - header.height(),
    );
    let view = key(("studio-panel", page));
    b.scroll(view, body, app.nav.scroll(view), Action::None, |b, top| {
        let mut c = crate::ui::Col {
            x: b.px(16.0),
            y: top + b.px(4.0),
            w: body.width() - b.px(32.0),
            view,
        };
        for id in crate::docs::DocId::ALL {
            if let Some(error) = app.docs.error(id) {
                controls::notice(b, &mut c, &format!("{}: {error}", id.label()));
            }
        }
        pages::view(app, b, &mut c, page);
        c.y + b.px(16.0)
    });
    pages::toast(app, &mut b, window);
    Built {
        list: b.list,
        targets: b.targets,
        scrolls: b.scrolls,
        chrome: vec![(header, Chrome::Caption)],
        regions: vec![header, body],
        overlay_from: usize::MAX,
    }
}

/// Reserve a top-bar band and 96 DIPs for a floating dock, within the primary work area.
pub fn placement(app: &App) -> (i32, i32, i32, i32) {
    let monitor = app.display();
    let fallback = mypaper_desktop::Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    let work = monitor.map_or(fallback, |m| m.work);
    let screen = monitor.map_or(fallback, |m| m.rect);
    let scale = monitor.map_or(app.scale, |m| m.dpi as f32 / 96.0);
    bounds(screen, work, scale)
}

/// `Surface::place` sizes the outer Win32 frame. Compensate for its invisible resize
/// borders so the opaque client panel itself is exactly 480 (or 360) DIPs wide.
pub fn fit_client(surface: &mypaper_ui::Surface, bounds: (i32, i32, i32, i32)) {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let mut outer = RECT::default();
    let mut client = POINT::default();
    // SAFETY: read-only geometry queries on our own window into local out-pointers.
    let queried = unsafe {
        GetWindowRect(surface.hwnd(), &mut outer).is_ok()
            && ClientToScreen(surface.hwnd(), &mut client).as_bool()
    };
    if !queried {
        return;
    }
    let size = surface.client_size();
    surface.place((
        bounds.0 - (client.x - outer.left),
        bounds.1 - (client.y - outer.top),
        bounds.2 + (outer.right - outer.left - size.0 as i32),
        bounds.3 + (outer.bottom - outer.top - size.1 as i32),
    ));
}

fn bounds(
    screen: mypaper_desktop::Rect,
    work: mypaper_desktop::Rect,
    scale: f32,
) -> (i32, i32, i32, i32) {
    let px = |v: f32| (v * scale).round() as i32;
    let width = px(if (screen.width as f32 / scale) < 1400.0 {
        360.0
    } else {
        480.0
    })
    .min((work.width - px(16.0)).max(1));
    let top = work.y.max(screen.y + px(40.0));
    let bottom = work.y + work.height - px(96.0);
    (
        work.x + work.width - px(8.0) - width,
        top,
        width,
        (bottom - top).max(1),
    )
}

pub fn round_window(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    // SAFETY: the DWM attribute is sized and belongs to our own opaque App window. The App
    // kind retains Windows' normal frame/shadow and is never made topmost.
    let preference = DWMWCP_ROUND;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const preference).cast(),
            size_of_val(&preference) as u32,
        )
    };
}
