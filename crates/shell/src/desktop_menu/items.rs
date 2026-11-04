//! The desktop menu's items: which there are (from `desktop_menu` in the config and the
//! desktop's current state) and what each does. Pure, so it is tested without a window; the
//! menu itself (layout, drawing, input) is `mypaper_ui::Menu`.

use mypaper_config::desktop_menu::{Builtin, DesktopMenu, Entry as Configured};
use mypaper_desktop::desktop_view::{IconSize, NewItem, SortKey};
use mypaper_ui::Entry;

/// What choosing an item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    IconSize(IconSize),
    ToggleIcons,
    SortBy(SortKey),
    SortOrder {
        ascending: bool,
    },
    Refresh,
    New(NewItem),
    Paste,
    OpenStudio,
    EditWidgets,
    Display,
    Personalize,
    Terminal,
    /// Hand the right-click to the native Windows menu.
    MoreOptions,
    Custom {
        command: String,
        args: Vec<String>,
    },
}

pub type Item = Entry<Command>;

/// What the desktop looks like right now: drives the check marks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewState {
    pub icon_size: Option<IconSize>,
    pub icons_visible: bool,
    /// The sort column and whether it is ascending.
    pub sort: Option<(SortKey, bool)>,
    pub can_paste: bool,
}

fn sort_label(key: SortKey) -> &'static str {
    match key {
        SortKey::Name => "Name",
        SortKey::Size => "Size",
        SortKey::Type => "Item type",
        SortKey::Modified => "Date modified",
    }
}

/// The menu for `config` (hidden items left out, no stray separators) in `state`.
pub fn build(config: &DesktopMenu, state: ViewState) -> Vec<Item> {
    config
        .tidy_items()
        .into_iter()
        .map(|entry| match entry {
            Configured::Custom(c) => Entry::action(
                &c.label,
                Command::Custom {
                    command: c.command.clone(),
                    args: c.args.clone(),
                },
            ),
            Configured::Builtin(builtin) => builtin_item(*builtin, state),
        })
        .collect()
}

fn builtin_item(builtin: Builtin, state: ViewState) -> Item {
    match builtin {
        Builtin::Separator => Entry::Separator,
        Builtin::View => {
            let size = |label, size| {
                Entry::check(
                    label,
                    Command::IconSize(size),
                    state.icon_size == Some(size),
                )
            };
            Entry::Submenu {
                label: "View".into(),
                items: vec![
                    size("Large icons", IconSize::Large),
                    size("Medium icons", IconSize::Medium),
                    size("Small icons", IconSize::Small),
                    Entry::Separator,
                    Entry::check(
                        "Show desktop icons",
                        Command::ToggleIcons,
                        state.icons_visible,
                    ),
                ],
            }
        }
        Builtin::Sort => {
            let current = state.sort;
            let mut items: Vec<Item> = SortKey::ALL
                .into_iter()
                .map(|key| {
                    let on = current.is_some_and(|(k, _)| k == key);
                    Entry::check(sort_label(key), Command::SortBy(key), on)
                })
                .collect();
            items.push(Entry::Separator);
            let ascending = current.is_none_or(|(_, up)| up);
            items.push(Entry::check(
                "Ascending",
                Command::SortOrder { ascending: true },
                ascending,
            ));
            items.push(Entry::check(
                "Descending",
                Command::SortOrder { ascending: false },
                !ascending,
            ));
            Entry::Submenu {
                label: "Sort by".into(),
                items,
            }
        }
        Builtin::Refresh => Entry::action("Refresh", Command::Refresh),
        Builtin::New => Entry::Submenu {
            label: "New".into(),
            items: vec![
                Entry::action("Folder", Command::New(NewItem::Folder)),
                Entry::action("Text Document", Command::New(NewItem::TextDocument)),
            ],
        },
        Builtin::Paste => Entry::Action {
            label: "Paste".into(),
            command: Command::Paste,
            checked: None,
            enabled: state.can_paste,
        },
        Builtin::Wallpaper => Entry::action("Change wallpaper", Command::OpenStudio),
        Builtin::Widgets => Entry::action("Edit widgets", Command::EditWidgets),
        Builtin::Display => Entry::action("Display settings", Command::Display),
        Builtin::Personalize => Entry::action("Personalize", Command::Personalize),
        Builtin::Terminal => Entry::action("Open terminal here", Command::Terminal),
        Builtin::More => Entry::action("Show more options", Command::MoreOptions),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_config::desktop_menu::Custom;
    use mypaper_ui::{Tokens, Ui, icons, menu};

    fn config(items: Vec<Configured>) -> DesktopMenu {
        DesktopMenu {
            enabled: true,
            items,
        }
    }

    fn labels(items: &[Item]) -> Vec<&str> {
        items.iter().map(Item::label).collect()
    }

    #[test]
    fn the_default_menu_has_the_documented_items_in_order() {
        let items = build(&DesktopMenu::default(), ViewState::default());
        assert_eq!(
            labels(&items),
            [
                "View",
                "Sort by",
                "Refresh",
                "",
                "New",
                "Paste",
                "",
                "Change wallpaper",
                "Edit widgets",
                "Display settings",
                "Personalize",
                "",
                "Open terminal here",
                "Show more options",
            ]
        );
    }

    #[test]
    fn hidden_items_leave_no_gap_and_custom_commands_carry_their_arguments() {
        let items = build(
            &config(vec![
                Configured::Builtin(Builtin::Separator),
                Configured::Builtin(Builtin::Refresh),
                Configured::Builtin(Builtin::Separator),
                Configured::Builtin(Builtin::Separator),
                Configured::Custom(Custom {
                    label: "Notes".into(),
                    command: "notepad.exe".into(),
                    args: vec!["a.txt".into()],
                }),
            ]),
            ViewState::default(),
        );
        assert_eq!(labels(&items), ["Refresh", "", "Notes"]);
        assert!(matches!(
            &items[2],
            Entry::Action { command: Command::Custom { command, args }, .. }
                if command == "notepad.exe" && args == &["a.txt"]
        ));
    }

    #[test]
    fn check_marks_follow_the_desktops_state() {
        let state = ViewState {
            icon_size: Some(IconSize::Medium),
            icons_visible: true,
            sort: Some((SortKey::Modified, false)),
            can_paste: false,
        };
        let items = build(&DesktopMenu::default(), state);
        let marks = |item: &Item| -> Vec<(String, Option<bool>)> {
            let Entry::Submenu { items, .. } = item else {
                panic!("a submenu")
            };
            items
                .iter()
                .filter_map(|i| match i {
                    Entry::Action { label, checked, .. } => Some((label.clone(), *checked)),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            marks(&items[0]),
            [
                ("Large icons".to_owned(), Some(false)),
                ("Medium icons".to_owned(), Some(true)),
                ("Small icons".to_owned(), Some(false)),
                ("Show desktop icons".to_owned(), Some(true)),
            ]
        );
        let sort = marks(&items[1]);
        assert!(sort.contains(&("Date modified".to_owned(), Some(true))));
        assert!(sort.contains(&("Name".to_owned(), Some(false))));
        assert!(sort.contains(&("Descending".to_owned(), Some(true))));
        assert!(sort.contains(&("Ascending".to_owned(), Some(false))));

        // Paste is disabled with nothing to paste.
        let paste = items.iter().find(|i| i.label() == "Paste").unwrap();
        assert!(!paste.selectable());
    }

    /// The default menu as it opens, drawn headless, against its golden PNGs (the root with
    /// "Refresh" hovered, and the View submenu): the same pixels the shell puts on screen.
    #[test]
    fn the_default_menu_matches_its_golden_images() {
        let mut ui = Ui::new().unwrap();
        let tokens = Tokens {
            icons: icons::system_family(&ui.text),
            ..Tokens::default()
        };
        let state = ViewState {
            icon_size: Some(IconSize::Medium),
            icons_visible: true,
            sort: Some((SortKey::Name, true)),
            can_paste: false,
        };
        let root = build(&DesktopMenu::default(), state);
        let Entry::Submenu { items: view, .. } = &root[0] else {
            panic!("View is a submenu")
        };
        let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
        for (name, items, hover) in [("menu-root", &root, Some(2)), ("menu-view", view, None)] {
            let layout = menu::layout(items, &tokens, 1.0, &mut ui.text);
            let list = menu::view(items, &layout, hover, &tokens);
            let edge = tokens.menu.shadow.ceil() as u32;
            let (w, h) = layout.size();
            let (w, h) = (w as u32, h as u32);
            let image = mypaper_ui::render(&mut ui, &list, w + 2 * edge, h + 2 * edge).unwrap();
            // Clicks hit every pixel of the body but its rounded corners (a layered window
            // passes alpha 0 through), and the far corner of the shadow margin is clear.
            let r = tokens.radius.ceil() as u32;
            for y in edge..edge + h {
                for x in edge..edge + w {
                    let corner =
                        (x < edge + r || x >= edge + w - r) && (y < edge + r || y >= edge + h - r);
                    assert!(
                        corner || image.pixel(x, y)[3] > 0,
                        "({x}, {y}) lets clicks through"
                    );
                }
            }
            assert_eq!(image.pixel(0, 0)[3], 0);
            mypaper_ui::golden::check(&image, &golden.join(format!("{name}.png"))).unwrap();
        }
    }
}
