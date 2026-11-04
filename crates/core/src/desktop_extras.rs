//! Desktop extras (docs/desktop-extras.md), hosted in core because core is long-lived
//! and idle: mypaper's own right-click menu for the bare desktop, the opt-in Win11
//! "classic context menu" switch, and the desktop view helpers the tray uses.
//!
//! Both options follow `config.json` live. The right-click menu runs on two threads of its
//! own (see `mypaper_desktop::desktop_menu`), so core's supervisor loop is never in the path
//! of a mouse event.

use std::sync::mpsc;
use std::time::Duration;

use mypaper_config::Config;
use mypaper_desktop::desktop_menu::{AppAction, DesktopMenu, Look, Settings};
use mypaper_desktop::desktop_view::{DesktopView, IconSize};
use mypaper_theme::{Appearance, Color, LoadedTheme, Paint, Surface, TextTone};
use mypaper_tweaks::classic_menu::{Change, ClassicMenu};
use mypaper_tweaks::palette;
use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW,
};
use windows::core::{HSTRING, w};

use crate::tray;

/// How long the tray waits for Explorer to describe the desktop before showing its
/// menu without that part (a hung Explorer must not hang the supervisor).
const VIEW_TIMEOUT: Duration = Duration::from_millis(400);

pub struct DesktopExtras {
    menu: Option<DesktopMenu>,
    classic: Option<ClassicMenu>,
}

impl DesktopExtras {
    /// Starts what `config` asks for. Nothing runs, and no hook is installed, unless
    /// `desktop_menu.enabled`.
    pub fn new(config: &Config) -> Self {
        let classic = ClassicMenu::new()
            .inspect_err(|e| log::warn!("classic menu switch unavailable: {e:#}"))
            .ok();
        let mut extras = Self {
            menu: None,
            classic,
        };
        extras.apply(config);
        extras
    }

    /// `config.json` or the theme's look changed: bring both options in line.
    pub fn follow_config(&mut self) {
        match Config::load() {
            Ok(config) => self.apply(&config),
            Err(e) => log::warn!("desktop extras unchanged: {e:#}"),
        }
    }

    /// Drops the menu (and with it the hook). The classic-menu setting stays as the user left it.
    pub fn stop(&mut self) {
        self.menu = None;
    }

    fn apply(&mut self, config: &Config) {
        self.apply_menu(config);
        self.apply_classic(config.classic_context_menu);
    }

    fn apply_menu(&mut self, config: &Config) {
        if !config.desktop_menu.enabled {
            self.menu = None;
            return;
        }
        let settings = Settings {
            menu: config.desktop_menu.clone(),
            look: look_for(&config.theme),
        };
        match &self.menu {
            Some(menu) => menu.update(settings),
            None => match DesktopMenu::start(settings, run_app_action) {
                Ok(menu) => {
                    log::info!("desktop right-click menu on");
                    self.menu = Some(menu);
                }
                Err(e) => log::warn!("desktop right-click menu not started: {e:#}"),
            },
        }
    }

    /// Turns the classic context menu on or off to match the config. Explorer reads the
    /// setting when it starts, so a real change gets a notice (never an automatic restart).
    fn apply_classic(&mut self, wanted: bool) {
        let Some(classic) = &mut self.classic else {
            return;
        };
        match classic.set(wanted) {
            Ok(Change::Applied) => {
                log::info!("classic context menu {}", if wanted { "on" } else { "off" });
                notify_restart_explorer(wanted);
            }
            Ok(Change::Unchanged) => {}
            Err(e) => log::warn!("classic context menu not changed: {e:#}"),
        }
    }
}

/// The items that belong to the rest of mypaper.
fn run_app_action(action: AppAction) {
    match action {
        AppAction::OpenStudio => tray::open_studio(),
        AppAction::EditWidgets => tray::toggle_widget_edit_mode(),
    }
}

/// Tells the user the classic menu change needs an Explorer restart. Shown on its own thread
/// so core's supervisor loop is not held up by the dialog.
fn notify_restart_explorer(classic: bool) {
    let state = if classic { "on" } else { "off" };
    let text = format!(
        "The classic Windows context menu is now {state}.\n\n\
         Explorer reads this setting when it starts: restart Explorer (Task Manager, \
         Windows Explorer, Restart) or sign out and back in to see it.\n\n\
         mypaper never restarts Explorer for you."
    );
    let spawned = std::thread::Builder::new()
        .name("classic-menu-notice".into())
        .spawn(move || {
            let flags = MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST;
            // SAFETY: both strings outlive the call, which blocks this thread until dismissed.
            unsafe { MessageBoxW(None, &HSTRING::from(text), w!("mypaper"), flags) };
        });
    if let Err(e) = spawned {
        log::warn!("could not show the Explorer restart notice: {e}");
    }
}

/// The menu's look: the built-in defaults under the active theme's `bars` appearance
/// (the same variables the bars use), with `accent: "auto"` taken from the wallpaper palette.
/// `blur` has no counterpart: the menu is drawn as a nearly opaque tint.
fn look_for(theme_name: &str) -> Look {
    let loaded = mypaper_theme::locate(Some(theme_name))
        .and_then(LoadedTheme::load)
        .inspect_err(|e| log::debug!("desktop menu look: {e:#}"))
        .ok();
    let theme_look = loaded
        .as_ref()
        .and_then(|t| t.theme.bars.as_ref())
        .and_then(|skin| skin.appearance.clone())
        .unwrap_or_default();
    let auto_accent = loaded.as_ref().and_then(|t| {
        palette::read()
            .filter(|snapshot| snapshot.is_for(&t.dir))
            .map(|snapshot| snapshot.palette.accent)
    });
    resolve_look(&theme_look, auto_accent)
}

/// The menu's built-in appearance.
fn default_appearance() -> Appearance {
    Appearance {
        surface: Some(Surface::Glass),
        tint: Some(Color([0x16, 0x18, 0x1d])),
        opacity: Some(0.94),
        radius: Some(10.0),
        border: Some(true),
        text: Some(TextTone::Light),
        accent: Some(Paint::Color(Color([0xff; 3]))),
        font: Some("Segoe UI Variable Text, Segoe UI".into()),
        scale: Some(1.0),
        shadow: Some(true),
        blur: None,
    }
}

fn resolve_look(theme: &Appearance, auto_accent: Option<Color>) -> Look {
    let merged = theme.over(&default_appearance());
    let fallback = Look::default();
    Look {
        dark: merged.text.unwrap_or(TextTone::Light) == TextTone::Light,
        tint: merged.tint.map_or(fallback.tint, |c| c.0),
        opacity: merged.opacity.unwrap_or(fallback.opacity),
        radius: merged.radius.unwrap_or(fallback.radius),
        border: merged.border.unwrap_or(fallback.border),
        accent: match merged.accent {
            Some(Paint::Color(color)) => color.0,
            Some(Paint::Auto) | None => auto_accent.map_or(fallback.accent, |c| c.0),
        },
        font: merged.font.unwrap_or(fallback.font),
        scale: merged.scale.unwrap_or(fallback.scale),
        shadow: merged.shadow.unwrap_or(fallback.shadow),
    }
}

/// The desktop's icon size and whether its icons are shown, for the tray's check marks.
/// `None` when Explorer does not answer in time.
pub fn desktop_state() -> Option<(IconSize, bool)> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("desktop-state".into())
        .spawn(move || {
            let state = DesktopView::open()
                .and_then(|view| Ok((view.icon_size()?, view.icons_visible()?)))
                .inspect_err(|e| log::debug!("desktop state: {e:#}"))
                .ok();
            let _ = tx.send(state);
        })
        .ok()?;
    rx.recv_timeout(VIEW_TIMEOUT).ok().flatten()
}

/// Sets the desktop's icon size (on a thread of its own: Explorer answers over COM).
pub fn set_icon_size(size: IconSize) {
    on_desktop_view("icon size", move |view| view.set_icon_size(size));
}

/// Shows or hides the desktop icons.
pub fn set_icons_visible(visible: bool) {
    on_desktop_view("desktop icons", move |view| view.set_icons_visible(visible));
}

fn on_desktop_view(
    what: &'static str,
    action: impl FnOnce(&DesktopView) -> anyhow::Result<()> + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("desktop-view".into())
        .spawn(move || {
            if let Err(e) = DesktopView::open().and_then(|view| action(&view)) {
                log::warn!("could not change the {what}: {e:#}");
            }
        });
    if let Err(e) = spawned {
        log::warn!("could not change the {what}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_theme_look_the_defaults_apply() {
        let look = resolve_look(&Appearance::default(), None);
        assert_eq!(look, Look::default());
    }

    #[test]
    fn the_themes_bars_appearance_wins_and_auto_accent_follows_the_palette() {
        let theme = Appearance {
            tint: Some(Color([0xf0, 0xf0, 0xf2])),
            text: Some(TextTone::Dark),
            radius: Some(4.0),
            accent: Some(Paint::Auto),
            font: Some("Inter".into()),
            shadow: Some(false),
            ..Default::default()
        };
        let look = resolve_look(&theme, Some(Color([0x19, 0xcd, 0x86])));
        assert!(!look.dark, "dark text means a light menu");
        assert_eq!(look.tint, [0xf0, 0xf0, 0xf2]);
        assert_eq!(look.radius, 4.0);
        assert_eq!(look.accent, [0x19, 0xcd, 0x86]);
        assert_eq!(look.font, "Inter");
        assert!(!look.shadow);
        // Unset fields keep the defaults.
        assert_eq!(look.opacity, Look::default().opacity);
        // `auto` without a palette falls back to the default accent.
        let waiting = resolve_look(&theme, None);
        assert_eq!(waiting.accent, Look::default().accent);
    }
}
