//! The shell's views for headless previews (Studio): the same display lists the live surfaces
//! present, built from a sample model instead of live services. Nothing here starts a service,
//! a hook or a window.

/// The top bar and dock.
pub mod bars {
    pub use crate::bars::icons::Images;
    pub use crate::bars::look::Tokens;
    pub use crate::bars::services::{Model, default_pins};
    pub use crate::bars::view::{Frame, Target, dock, top};

    use mypaper_bars::{audio::Volume, custom::Custom, desktops::Workspaces, wake::Waker};
    use windows::Win32::Foundation::SYSTEMTIME;

    /// A calm, plausible model for previews: a few workspaces, the given clock, volume at half,
    /// no media or tray icons (they're the user's own and arrive live on the desktop).
    pub fn sample_model(clock: SYSTEMTIME, app: &str) -> Model {
        let (tx, _) = std::sync::mpsc::channel();
        Model {
            foreground: Some(1),
            app_name: app.into(),
            app_icon: None,
            media: None,
            volume: Some(Volume {
                level: 0.5,
                muted: false,
            }),
            network: None,
            battery: None,
            stats: None,
            workspaces: Workspaces {
                count: 3,
                current: 0,
            },
            language: "ENG".into(),
            notifications: None,
            tray: vec![],
            clock,
            custom: Custom::new(Waker::new(tx, 0)),
            trash_full: false,
        }
    }
}

/// The desktop widgets.
pub mod widgets {
    pub use crate::widgets::model::{Model, Task, Widget};
    pub use crate::widgets::time::Stamp;
    pub use crate::widgets::view::{Frame, view};

    /// The widgets' tokens from a merged appearance map (what the live host uses).
    pub fn tokens(appearance: &serde_json::Map<String, serde_json::Value>) -> mypaper_ui::Tokens {
        crate::widgets::tokens(appearance)
    }
}

/// The desktop right-click menu.
pub mod menu {
    pub use crate::desktop_menu::items::{Command, Item, ViewState, build};

    /// The menu's tokens for a theme (name or folder), as the live menu resolves them.
    pub fn tokens(theme: &str, text: &mypaper_ui::Text) -> mypaper_ui::Tokens {
        crate::look::tokens(theme, text)
    }
}
