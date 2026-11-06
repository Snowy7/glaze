//! Studio's transient selection client. The panel calls `connect` while building and
//! `on_key` for the semantic IPC wake; no native keyboard input is synthesized.
use mypaper_shell::studio_pick::wire::{self, Kind, Message, Selection};
use std::cell::RefCell;
use windows::Win32::Foundation::HWND;

use super::{Command, Page};
use crate::actions::{Action, Bar, Slot};
use crate::app::App;

struct Client {
    endpoint: wire::Endpoint,
    shell: Option<HWND>,
    panel: HWND,
    highlighted: Option<String>,
}

thread_local! { static CLIENT: RefCell<Option<Client>> = const { RefCell::new(None) }; }

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(shell) = self.shell {
            wire::send(shell, self.endpoint.hwnd(), &Message::End);
        }
    }
}

/// Starts only for a real panel window (fixtures never contact the desktop).
pub fn connect(app: &App) {
    if app.fixture {
        return;
    }
    let Some(panel) = app.window else { return };
    CLIENT.with(|client| {
        let mut client = client.borrow_mut();
        if client.as_ref().is_some_and(|c| c.panel != panel) {
            *client = None;
        }
        if client.is_none() {
            let Ok(endpoint) = wire::Endpoint::new(false) else {
                return;
            };
            endpoint.wake_ui(panel);
            *client = Some(Client {
                endpoint,
                shell: None,
                panel,
                highlighted: None,
            });
        }
        let client = client.as_mut().unwrap();
        let shell = wire::find_shell();
        if client.shell != shell {
            client.shell = shell.filter(|&shell| {
                wire::send(
                    shell,
                    client.endpoint.hwnd(),
                    &Message::Begin {
                        panel: panel.0 as isize,
                    },
                )
            });
            client.highlighted = None;
        }
    });
    let hovered = app
        .input
        .hover
        .and_then(|key| app.frame.targets.iter().find(|t| t.key == key))
        .and_then(|target| match &target.action {
            Action::SelectWidget(id) => id.clone(),
            Action::Panel(Command::Open(Page::Widget)) => app.nav.widget.clone(),
            _ => None,
        });
    highlight(hovered);
}

/// Also available to a row that has a different navigation action.
pub fn highlight(id: Option<String>) {
    CLIENT.with(|client| {
        if let Some(client) = client.borrow_mut().as_mut() {
            if client.highlighted == id {
                return;
            }
            if let Some(shell) = client.shell {
                wire::send(
                    shell,
                    client.endpoint.hwnd(),
                    &Message::Highlight { id: id.clone() },
                );
            }
            client.highlighted = id;
        }
    });
}

pub fn on_key(app: &mut App, key: u16) -> bool {
    if key != wire::PICK_EVENT {
        return false;
    }
    let selection = CLIENT.with(|client| {
        let client = client.borrow();
        let client = client.as_ref()?;
        let mut latest = None;
        while let Some((source, message)) = client.endpoint.next() {
            if Some(source) == client.shell
                && let Message::Selected { selection } = message
            {
                latest = Some(selection);
            }
        }
        latest
    });
    if let Some(selection) = selection {
        selected(app, selection);
    }
    true
}

/// The one navigation seam. Selection changes only navigation, never document values.
fn selected(app: &mut App, selection: Selection) {
    let previous_display = app.nav.display;
    if let Some(display) = app
        .monitors
        .iter()
        .position(|m| m.device == selection.display)
    {
        app.nav.display = display;
    }
    let page = match selection.kind {
        Kind::Widget => {
            app.nav.widget = Some(selection.id);
            Page::Widget
        }
        Kind::Wallpaper => Page::Wallpapers,
        Kind::Bar | Kind::Module => {
            let mut id = selection.id.split('/');
            let bar = if id.next() == Some("dock") {
                Bar::Dock
            } else {
                Bar::Top
            };
            app.nav.bar = Some(bar);
            app.nav.module = if selection.kind == Kind::Module {
                match (id.next(), id.next().and_then(|v| v.parse::<usize>().ok())) {
                    (Some(list @ ("left" | "center" | "right")), Some(index)) => Some(Slot {
                        bar,
                        list: match list {
                            "left" => "left",
                            "center" => "center",
                            _ => "right",
                        },
                        index,
                    }),
                    _ => None,
                }
            } else {
                None
            };
            if selection.kind == Kind::Module && app.nav.module.is_some() {
                Page::Module
            } else if bar == Bar::Dock {
                Page::Dock
            } else {
                Page::Top
            }
        }
    };
    if let Some(panel) = &mut app.panel {
        panel.page = page;
    }
    if app.nav.display != previous_display
        && let Some(window) = app.window
    {
        use windows::Win32::Foundation::{POINT, RECT};
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
        };
        let bounds = super::placement(app);
        let mut outer = RECT::default();
        let mut client = POINT::default();
        // SAFETY: only the panel's own window moves, preserving focus and the other windows.
        unsafe {
            if GetWindowRect(window, &mut outer).is_ok()
                && ClientToScreen(window, &mut client).as_bool()
            {
                let _ = SetWindowPos(
                    window,
                    None,
                    bounds.0 - (client.x - outer.left),
                    bounds.1 - (client.y - outer.top),
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
                );
            }
        }
    }
    app.input.focus = None;
    app.input.hover = None;
    app.dirty = true;
    log::info!("Studio desktop selection opened {page:?}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_ui::Ui;

    #[test]
    fn desktop_picks_navigate_without_editing_documents() {
        let ui = Ui::new().unwrap();
        let mut app = crate::goldens::app(true, &ui);
        app.panel = Some(crate::panel::State::default());
        let revision = app.docs.version;
        selected(
            &mut app,
            Selection {
                kind: Kind::Widget,
                id: "clock-2".into(),
                display: String::new(),
            },
        );
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Widget);
        assert_eq!(app.nav.widget.as_deref(), Some("clock-2"));
        selected(
            &mut app,
            Selection {
                kind: Kind::Wallpaper,
                id: "wallpaper".into(),
                display: String::new(),
            },
        );
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Wallpapers);
        selected(
            &mut app,
            Selection {
                kind: Kind::Module,
                id: "top/right/2".into(),
                display: String::new(),
            },
        );
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Module);
        assert_eq!(
            app.nav.module,
            Some(Slot {
                bar: Bar::Top,
                list: "right",
                index: 2
            })
        );
        selected(
            &mut app,
            Selection {
                kind: Kind::Bar,
                id: "dock".into(),
                display: String::new(),
            },
        );
        assert_eq!(app.panel.as_ref().unwrap().page, Page::Dock);
        assert!(app.nav.module.is_none());
        assert_eq!(app.docs.version, revision);
    }
}
