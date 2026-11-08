//! The desktop menu's items: which there are (from `desktop_menu` in the config, the desktop's
//! current state and the shell's own menu, [`super::shell_menu`]) and what each does. Pure, so
//! it is tested without a window; the menu itself (layout, drawing, input) is `glaze_ui::Menu`.
//!
//! The default menu is Windows 11's full desktop menu, with nothing behind "Show more options":
//! View, Sort by, Refresh | Paste | Open in Terminal and the shell extensions | New (every
//! registered type) | Display settings, Personalize | Glaze's own items. Every row has an icon:
//! the extension's own bitmap, else the Segoe Fluent glyph Windows 11 uses.

use std::sync::Arc;

use glaze_config::desktop_menu::{Builtin, DesktopMenu, Entry as Configured};
use glaze_desktop::desktop_view::{IconSize, NewItem, SortKey};
use glaze_ui::{Entry, Image, MenuIcon};

use super::shell_menu::{ShellIcon, ShellItem, Snapshot};

/// Segoe Fluent Icons glyphs of Windows 11's own desktop menu.
pub mod glyph {
    pub const VIEW: char = '\u{E8A9}';
    pub const SORT: char = '\u{E8CB}';
    pub const REFRESH: char = '\u{E72C}';
    pub const NEW: char = '\u{ECC8}';
    pub const PASTE: char = '\u{E77F}';
    pub const DISPLAY: char = '\u{E7F4}';
    pub const PERSONALIZE: char = '\u{E771}';
    pub const TERMINAL: char = '\u{E756}';
    pub const FOLDER: char = '\u{E8B7}';
    pub const DOCUMENT: char = '\u{E8A5}';
    /// An extension that brings no icon of its own.
    pub const APP: char = '\u{ECAA}';
    // Glaze's own items, each its own glyph (Photo, Tiles, Add, Color), apart from Display
    // settings' monitor, Personalize's brush, View's even grid and New's circled plus.
    pub const WALLPAPER: char = '\u{EB9F}';
    pub const WIDGETS: char = '\u{ECA5}';
    pub const ADD: char = '\u{E710}';
    pub const STUDIO: char = '\u{E790}';
}

/// What choosing an item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    IconSize(IconSize),
    ToggleIcons,
    AutoArrange(bool),
    AlignToGrid(bool),
    SortBy(SortKey),
    SortOrder {
        ascending: bool,
    },
    Refresh,
    New(NewItem),
    Paste,
    OpenStudio,
    EditWidgets,
    AddWidget,
    Display,
    Personalize,
    Terminal,
    /// Command `id` of the shell's menu `generation` ([`super::shell_menu`]).
    Shell {
        generation: u64,
        id: u32,
    },
    /// Hand the right-click to the native Windows menu.
    MoreOptions,
    Custom {
        command: String,
        args: Vec<String>,
    },
    /// A component plugin's `menu` item (`desktop-menu`), sent to it as `event::command`.
    Plugin {
        plugin: String,
        id: String,
    },
}

pub type Item = Entry<Command>;

/// What the desktop looks like right now: drives the check marks and radio dots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewState {
    pub icon_size: Option<IconSize>,
    pub icons_visible: bool,
    pub auto_arrange: bool,
    pub align_to_grid: bool,
    /// The sort column and whether it is ascending.
    pub sort: Option<(SortKey, bool)>,
    pub can_paste: bool,
}

/// What the menu is built from besides the config and the desktop's state.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sources<'a> {
    /// The shell's own menu, once the worker has built it.
    pub shell: Option<&'a Snapshot>,
    /// Glaze's icon, for Glaze's items only where the icon font lacks their glyphs.
    pub Glaze: Option<&'a Arc<Image>>,
    /// Plugins' menu items (`crate::widgets::plugin_events::menu_items`), already gated.
    pub plugins: &'a [crate::widgets::plugin_events::Contribution],
}

/// A Segoe Fluent glyph for a plugin's kit icon name; other names get the app glyph.
fn plugin_glyph(icon: Option<&str>) -> char {
    match icon {
        Some("sparkles" | "star") => '\u{E734}',
        Some("cloud" | "weather") => '\u{E753}',
        Some("globe" | "network") => '\u{E774}',
        Some("code" | "terminal") => '\u{E943}',
        Some("music" | "media") => '\u{E8D6}',
        Some("chat" | "message") => '\u{E8BD}',
        _ => glyph::APP,
    }
}

/// One submenu per plugin, under its name and icon, holding its items in manifest order.
fn plugin_items(plugins: &[crate::widgets::plugin_events::Contribution]) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for (n, item) in plugins.iter().enumerate() {
        if n > 0 && plugins[n - 1].plugin == item.plugin {
            continue;
        }
        let items = plugins[n..].iter().take_while(|c| c.plugin == item.plugin).map(|c| Entry::action(&c.label, Command::Plugin { plugin: c.plugin.clone(), id: c.id.clone() })).collect();
        out.push(Entry::Submenu { label: item.name.clone(), items }.with_icon(MenuIcon::Glyph(plugin_glyph(item.icon.as_deref()))));
    }
    out
}

fn sort_label(key: SortKey) -> &'static str {
    match key {
        SortKey::Name => "Name",
        SortKey::Size => "Size",
        SortKey::Type => "Item type",
        SortKey::Modified => "Date modified",
    }
}

/// One slot of the configured menu, in the same order Studio edits it.
enum Slot<'a> {
    Builtin(Builtin),
    Custom(&'a glaze_config::desktop_menu::Custom),
}

/// The menu for `config` in `state` with Glaze's built-ins only (Studio's preview).
pub fn build(config: &DesktopMenu, state: ViewState) -> Vec<Item> {
    build_with(config, state, Sources::default())
}

/// The menu for `config` (hidden items left out, no stray separators) in `state`. With the
/// default config it is Windows 11's full menu; in a configured one, `more` stands for the shell
/// extensions and `new` lists every registered type.
pub fn build_with(config: &DesktopMenu, state: ViewState, sources: Sources) -> Vec<Item> {
    let slots = config
            .tidy_items()
            .into_iter()
            .map(|entry| match entry {
                Configured::Builtin(b) => Slot::Builtin(*b),
                Configured::Custom(c) => Slot::Custom(c),
            })
            .collect::<Vec<_>>();
    let Glaze = |glyph| match sources.Glaze {
        Some(image) => MenuIcon::Image(image.clone()),
        None => MenuIcon::Glyph(glyph),
    };
    let mut items = Vec::new();
    for slot in slots {
        match slot {
            Slot::Builtin(Builtin::More) => items.extend(extensions(sources.shell)),
            Slot::Builtin(b @ Builtin::Wallpaper) => {
                items.push(builtin_item(b, state, sources).with_icon(Glaze(glyph::WALLPAPER)))
            }
            Slot::Builtin(b @ Builtin::Widgets) => {
                items.push(builtin_item(b, state, sources).with_icon(Glaze(glyph::WIDGETS)))
            }
            Slot::Builtin(b @ Builtin::AddWidget) => {
                items.push(builtin_item(b, state, sources).with_icon(Glaze(glyph::ADD)))
            }
            Slot::Builtin(b @ Builtin::Studio) => {
                items.push(builtin_item(b, state, sources).with_icon(Glaze(glyph::STUDIO)))
            }
            Slot::Builtin(b) => items.push(builtin_item(b, state, sources)),
            Slot::Custom(c) => items.push(
                Entry::action(
                    &c.label,
                    Command::Custom {
                        command: c.command.clone(),
                        args: c.args.clone(),
                    },
                )
                .with_icon(MenuIcon::Glyph(glyph::APP)),
            ),
        }
    }
    if !sources.plugins.is_empty() {
        items.push(Entry::Separator);
        items.extend(plugin_items(sources.plugins));
    }
    tidy(group_open(items))
}

/// Gathers the top-level "Open …" items (Open in Terminal, "Open with Visual Studio", "Open Git
/// Bash here", …) into one "Open in" submenu where the first of them stood, each relabelled to
/// what it opens ("Terminal", "Visual Studio", "Git Bash"). One such item stays as it is.
fn group_open(items: Vec<Item>) -> Vec<Item> {
    let opens = |item: &Item| short_open_label(item.label()).is_some();
    if items.iter().filter(|item| opens(item)).count() < 2 {
        return items;
    }
    let mut out = Vec::with_capacity(items.len());
    let mut group: Vec<Item> = Vec::new();
    let mut at = None;
    for item in items {
        if let Some(short) = short_open_label(item.label()) {
            at.get_or_insert(out.len());
            let item = relabel(item, short);
            // Two of the same ("Terminal": Glaze's and Windows Terminal's own handler): keep
            // the shell's, which opens exactly what the user installed.
            match group.iter().position(|g| g.label() == item.label()) {
                Some(i) if is_shell(&item) && !is_shell(&group[i]) => group[i] = item,
                Some(_) => {}
                None => group.push(item),
            }
        } else {
            out.push(item);
        }
    }
    let submenu = Entry::Submenu {
        label: "Open in".into(),
        items: group,
    }
    .with_icon(MenuIcon::Glyph(glyph::TERMINAL));
    out.insert(at.unwrap_or(out.len()), submenu);
    out
}

/// Does this item run through the shell's own menu (an installed extension's verb)?
fn is_shell(item: &Item) -> bool {
    matches!(
        item.plain(),
        Entry::Action {
            command: Command::Shell { .. },
            ..
        }
    )
}

/// "Open in Terminal" → "Terminal", "Open with Visual Studio" → "Visual Studio", "Open Git Bash
/// here" → "Git Bash", "Open Warp in new tab" → "Warp in new tab"; `None` if it isn't an
/// "Open …" item (or is a bare "Open").
fn short_open_label(label: &str) -> Option<String> {
    let rest = label.strip_prefix("Open ")?;
    let rest = ["in ", "with "]
        .iter()
        .find_map(|p| rest.strip_prefix(p))
        .unwrap_or(rest);
    let rest = rest.strip_suffix(" here").unwrap_or(rest).trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

/// The same entry under a new label, keeping its icon, command and state.
fn relabel(item: Item, new: String) -> Item {
    match item {
        Entry::Iconic { icon, item } => Entry::Iconic {
            icon,
            item: Box::new(relabel(*item, new)),
        },
        Entry::Action {
            command,
            checked,
            enabled,
            ..
        } => Entry::Action {
            label: new,
            command,
            checked,
            enabled,
        },
        Entry::Submenu { items, .. } => Entry::Submenu { label: new, items },
        other => other,
    }
}

/// No leading, trailing or doubled separators.
fn tidy(items: Vec<Item>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    for item in items {
        if matches!(item, Entry::Separator) && matches!(out.last(), None | Some(Entry::Separator)) {
            continue;
        }
        out.push(item);
    }
    while matches!(out.last(), Some(Entry::Separator)) {
        out.pop();
    }
    out
}

/// Verbs of the shell's menu that Glaze's own items already cover.
const COVERED: [&str; 5] = ["display", "personalize", "refresh", "paste", "properties"];

/// Is this the shell's "New" submenu?
fn is_new(item: &ShellItem) -> bool {
    matches!(item, ShellItem::Submenu { items, .. }
        if items.iter().any(|i| matches!(i, ShellItem::Action { verb, .. } if verb == "newfolder")))
}

/// The shell's items besides "New" and the ones Glaze covers.
fn extensions(shell: Option<&Snapshot>) -> Vec<Item> {
    let Some(shell) = shell else {
        return Vec::new();
    };
    let covered = |item: &&ShellItem| matches!(item, ShellItem::Action { verb, .. } if COVERED.contains(&verb.as_str()));
    let items = shell
        .items
        .iter()
        .filter(|item| !is_new(item) && !covered(item))
        .map(|item| from_shell(item, shell.generation, glyph::APP))
        .collect();
    tidy(items)
}

fn from_shell(item: &ShellItem, generation: u64, fallback: char) -> Item {
    let icon = |icon: &Option<ShellIcon>| match icon {
        Some(icon) => MenuIcon::Image(icon.image.clone()),
        None => MenuIcon::Glyph(fallback),
    };
    match item {
        ShellItem::Separator => Entry::Separator,
        ShellItem::Action {
            label,
            id,
            icon: image,
            enabled,
            ..
        } => Entry::Action {
            label: label.clone(),
            command: Command::Shell {
                generation,
                id: *id,
            },
            checked: None,
            enabled: *enabled,
        }
        .with_icon(icon(image)),
        ShellItem::Submenu {
            label,
            icon: image,
            items,
        } => Entry::Submenu {
            label: label.clone(),
            items: items
                .iter()
                .map(|i| from_shell(i, generation, fallback))
                .collect(),
        }
        .with_icon(icon(image)),
    }
}

fn builtin_item(builtin: Builtin, state: ViewState, sources: Sources) -> Item {
    let icon = MenuIcon::Glyph;
    match builtin {
        Builtin::Separator => Entry::Separator,
        Builtin::View => {
            let size = |label, size| {
                Entry::radio(
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
                        "Auto arrange icons",
                        Command::AutoArrange(!state.auto_arrange),
                        state.auto_arrange,
                    ),
                    Entry::check(
                        "Align icons to grid",
                        Command::AlignToGrid(!state.align_to_grid),
                        state.align_to_grid,
                    ),
                    Entry::Separator,
                    Entry::check(
                        "Show desktop icons",
                        Command::ToggleIcons,
                        state.icons_visible,
                    ),
                ],
            }
            .with_icon(icon(glyph::VIEW))
        }
        Builtin::Sort => {
            let current = state.sort;
            let mut items: Vec<Item> = SortKey::ALL
                .into_iter()
                .map(|key| {
                    let on = current.is_some_and(|(k, _)| k == key);
                    Entry::radio(sort_label(key), Command::SortBy(key), on)
                })
                .collect();
            items.push(Entry::Separator);
            let ascending = current.is_none_or(|(_, up)| up);
            items.push(Entry::radio(
                "Ascending",
                Command::SortOrder { ascending: true },
                ascending,
            ));
            items.push(Entry::radio(
                "Descending",
                Command::SortOrder { ascending: false },
                !ascending,
            ));
            Entry::Submenu {
                label: "Sort by".into(),
                items,
            }
            .with_icon(icon(glyph::SORT))
        }
        Builtin::Refresh => {
            Entry::action("Refresh", Command::Refresh).with_icon(icon(glyph::REFRESH))
        }
        Builtin::New => {
            // The shell's own "New": Folder, Shortcut and every registered type, with icons.
            let shell_new = sources.shell.and_then(|shell| {
                let new = shell.items.iter().find(|i| is_new(i))?;
                Some((new, shell.generation))
            });
            let items = match shell_new {
                Some((ShellItem::Submenu { items, .. }, generation)) => items
                    .iter()
                    .map(|i| from_shell(i, generation, glyph::DOCUMENT))
                    .collect(),
                _ => vec![
                    Entry::action("Folder", Command::New(NewItem::Folder))
                        .with_icon(icon(glyph::FOLDER)),
                    Entry::action("Text Document", Command::New(NewItem::TextDocument))
                        .with_icon(icon(glyph::DOCUMENT)),
                ],
            };
            Entry::Submenu {
                label: "New".into(),
                items,
            }
            .with_icon(icon(glyph::NEW))
        }
        Builtin::Paste => Entry::Action {
            label: "Paste".into(),
            command: Command::Paste,
            checked: None,
            enabled: state.can_paste,
        }
        .with_icon(icon(glyph::PASTE)),
        Builtin::Wallpaper => Entry::action("Change wallpaper", Command::OpenStudio),
        Builtin::Widgets => Entry::action("Edit widgets", Command::EditWidgets),
        Builtin::AddWidget => Entry::action("Add widget", Command::AddWidget),
        Builtin::Studio => Entry::action("Glaze Studio", Command::OpenStudio),
        Builtin::Display => {
            Entry::action("Display settings", Command::Display).with_icon(icon(glyph::DISPLAY))
        }
        Builtin::Personalize => {
            Entry::action("Personalize", Command::Personalize).with_icon(icon(glyph::PERSONALIZE))
        }
        Builtin::Terminal => {
            Entry::action("Open in Terminal", Command::Terminal).with_icon(icon(glyph::TERMINAL))
        }
        Builtin::More => Entry::action("Show more options", Command::MoreOptions),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glaze_config::desktop_menu::Custom;
    use glaze_ui::{Tokens, Ui, icons, menu};

    fn config(items: Vec<Configured>) -> DesktopMenu {
        DesktopMenu {
            enabled: true,
            items,
        }
    }

    fn labels(items: &[Item]) -> Vec<&str> {
        items.iter().map(Item::label).collect()
    }

    /// A shell menu like this machine's: two extensions, "New", and verbs Glaze covers.
    fn shell() -> Snapshot {
        let action = |label: &str, verb: &str, id| ShellItem::Action {
            label: label.into(),
            verb: verb.into(),
            id,
            icon: None,
            enabled: true,
        };
        Snapshot {
            generation: 7,
            items: vec![
                action("Open Git Bash here", "git_shell", 75),
                ShellItem::Separator,
                action("NVIDIA App", "", 1),
                ShellItem::Separator,
                ShellItem::Submenu {
                    label: "New".into(),
                    icon: None,
                    items: vec![
                        action("Folder", "newfolder", 3),
                        action("Shortcut", "newlink", 4),
                        ShellItem::Separator,
                        action("Text Document", ".txt", 15),
                    ],
                },
                ShellItem::Separator,
                action("Display settings", "display", 81),
                action("Personalize", "personalize", 83),
            ],
            took: Default::default(),
        }
    }

    #[test]
    fn preview_order_is_the_editable_list_order_even_after_the_first_reorder() {
        use glaze_config::desktop_menu::default_items;
        let mut config = DesktopMenu::default();
        for _ in 0..2 {
            let expected: Vec<_> = config.tidy_items().into_iter().filter_map(|entry| {
                match entry {
                    Configured::Builtin(Builtin::More) => None, // Runtime extension slot.
                    Configured::Builtin(b) => Some(builtin_item(*b, ViewState::default(), Sources::default())),
                    Configured::Custom(_) => unreachable!(),
                }
            }).collect();
            assert_eq!(labels(&build(&config, ViewState::default())), labels(&tidy(expected)));
            let studio = config.items.pop().unwrap();
            config.items.insert(0, studio);
        }
        assert!(default_items().contains(&Configured::Builtin(Builtin::AddWidget)));
        assert!(default_items().contains(&Configured::Builtin(Builtin::Studio)));
    }

    #[test]
    fn existing_explicit_lists_keep_their_order_and_hidden_items() {
        let config: DesktopMenu = serde_json::from_str(r#"{"items":["refresh","new","paste","wallpaper","widgets","display","personalize","terminal","more"]}"#).unwrap();
        assert_eq!(labels(&build(&config, ViewState::default())), ["Refresh", "New", "Paste", "Change wallpaper", "Edit widgets", "Display settings", "Personalize", "Open in Terminal"]);
    }

    #[test]
    fn the_default_menu_is_windows_full_menu_then_Glazes() {
        let shell = shell();
        let sources = Sources { plugins: &[],
            shell: Some(&shell),
            Glaze: None,
        };
        let items = build_with(&DesktopMenu::default(), ViewState::default(), sources);
        assert_eq!(
            labels(&items),
            [
                "View",
                "Sort by",
                "Refresh",
                "",
                "Paste",
                "",
                "Open in",
                "",
                "NVIDIA App",
                "",
                "New",
                "",
                "Display settings",
                "Personalize",
                "",
                "Change wallpaper",
                "Edit widgets",
                "Add widget",
                "Glaze Studio",
            ]
        );
        // Every row has an icon; extensions run through the shell's own menu.
        assert!(
            items
                .iter()
                .all(|i| matches!(i, Entry::Separator) || i.icon().is_some())
        );
        // The "Open …" items share one submenu, labelled by what they open.
        let open = items.iter().find(|i| i.label() == "Open in").unwrap();
        let Entry::Submenu { items: open, .. } = open.plain() else {
            panic!("Open in is a submenu")
        };
        assert_eq!(labels(open), ["Terminal", "Git Bash"]);
        let git = open.iter().find(|i| i.label() == "Git Bash");
        assert!(matches!(
            git.map(Item::plain),
            Some(Entry::Action {
                command: Command::Shell {
                    generation: 7,
                    id: 75
                },
                ..
            })
        ));
        // "New" is the shell's, with every registered type.
        let new = items.iter().find(|i| i.label() == "New").unwrap();
        let Entry::Submenu { items: new, .. } = new.plain() else {
            panic!("New is a submenu")
        };
        assert_eq!(labels(new), ["Folder", "Shortcut", "", "Text Document"]);
    }

    #[test]
    fn an_installed_terminal_handler_replaces_Glazes_own() {
        let mut shell = shell();
        shell.items.insert(
            0,
            ShellItem::Action {
                label: "Open in Terminal".into(),
                verb: "openinterminal".into(),
                id: 90,
                icon: None,
                enabled: true,
            },
        );
        let sources = Sources { plugins: &[],
            shell: Some(&shell),
            Glaze: None,
        };
        let items = build_with(&DesktopMenu::default(), ViewState::default(), sources);
        let open = items.iter().find(|i| i.label() == "Open in").unwrap();
        let Entry::Submenu { items: open, .. } = open.plain() else {
            panic!("Open in is a submenu")
        };
        assert_eq!(labels(open), ["Terminal", "Git Bash"]);
        assert!(is_shell(&open[0]));
    }

    #[test]
    fn open_items_are_named_by_what_they_open() {
        let short = |s| short_open_label(s);
        assert_eq!(short("Open in Terminal").as_deref(), Some("Terminal"));
        assert_eq!(short("Open with Visual Studio").as_deref(), Some("Visual Studio"));
        assert_eq!(short("Open Git GUI here").as_deref(), Some("Git GUI"));
        assert_eq!(short("Open Warp in new tab").as_deref(), Some("Warp in new tab"));
        assert_eq!(short("Open"), None);
        assert_eq!(short("NVIDIA App"), None);
    }

    #[test]
    fn without_the_shells_menu_the_built_ins_open_at_once() {
        let items = build_with(
            &DesktopMenu::default(),
            ViewState::default(),
            Sources::default(),
        );
        assert!(labels(&items).starts_with(&["View", "Sort by", "Refresh", "", "Paste", ""]));
        let new = items.iter().find(|i| i.label() == "New").unwrap();
        let Entry::Submenu { items: new, .. } = new.plain() else {
            panic!("New is a submenu")
        };
        assert_eq!(labels(new), ["Folder", "Text Document"]);
    }

    #[test]
    fn hidden_items_leave_no_gap_and_custom_commands_carry_their_arguments() {
        let items = build_with(
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
            Sources::default(),
        );
        assert_eq!(labels(&items), ["Refresh", "", "Notes"]);
        assert!(matches!(
            items[2].plain(),
            Entry::Action { command: Command::Custom { command, args }, .. }
                if command == "notepad.exe" && args == &["a.txt"]
        ));
    }

    #[test]
    fn check_marks_and_radio_dots_follow_the_desktops_state() {
        let state = ViewState {
            icon_size: Some(IconSize::Medium),
            icons_visible: true,
            auto_arrange: false,
            align_to_grid: true,
            sort: Some((SortKey::Modified, false)),
            can_paste: false,
        };
        let items = build_with(&DesktopMenu::default(), state, Sources::default());
        let marks = |item: &Item| -> Vec<(String, Option<bool>, bool)> {
            let Entry::Submenu { items, .. } = item.plain() else {
                panic!("a submenu")
            };
            items
                .iter()
                .filter_map(|i| match i.plain() {
                    Entry::Action { label, checked, .. } => {
                        Some((label.clone(), *checked, i.icon().is_some()))
                    }
                    _ => None,
                })
                .collect()
        };
        let mark = |label: &str, on: bool, radio: bool| (label.to_owned(), Some(on), radio);
        assert_eq!(
            marks(&items[0]),
            [
                mark("Large icons", false, true),
                mark("Medium icons", true, true),
                mark("Small icons", false, true),
                mark("Auto arrange icons", false, false),
                mark("Align icons to grid", true, false),
                mark("Show desktop icons", true, false),
            ]
        );
        let sort = marks(&items[1]);
        assert!(sort.contains(&mark("Date modified", true, true)));
        assert!(sort.contains(&mark("Name", false, true)));
        assert!(sort.contains(&mark("Descending", true, true)));
        assert!(sort.contains(&mark("Ascending", false, true)));

        // Paste is disabled with nothing to paste.
        let paste = items.iter().find(|i| i.label() == "Paste").unwrap();
        assert!(!paste.selectable());
    }

    /// The default menu as it opens, drawn headless, against its golden PNGs (the root with
    /// "Refresh" hovered, and the View submenu): the same pixels the shell puts on screen.
    #[test]
    fn the_default_menu_matches_its_golden_images() {
        let mut ui = Ui::new().unwrap();
        // The bar theme's tokens, as the shell opens it (without the engine's glass here).
        let theme = glaze_ui::Theme::new(
            glaze_ui::Style::Liquid,
            glaze_ui::Scheme::Dark,
            glaze_ui::theme::SYSTEM_BLUE,
        );
        let tokens = Tokens {
            icons: icons::system_family(&ui.text),
            glass: None,
            ..theme.tokens()
        };
        let state = ViewState {
            icon_size: Some(IconSize::Medium),
            icons_visible: true,
            auto_arrange: false,
            align_to_grid: true,
            sort: Some((SortKey::Name, true)),
            can_paste: false,
        };
        let shell = shell();
        let sources = Sources { plugins: &[],
            shell: Some(&shell),
            Glaze: None,
        };
        let root = build_with(&DesktopMenu::default(), state, sources);
        let Entry::Submenu { items: view, .. } = root[0].plain() else {
            panic!("View is a submenu")
        };
        let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
        for (name, items, hover) in [("menu-root", &root, Some(2)), ("menu-view", view, None)] {
            let layout = menu::layout(items, &tokens, 1.0, &mut ui.text);
            let list = menu::view(items, &layout, hover, &tokens);
            let edge = tokens.menu.shadow.ceil() as u32;
            let (w, h) = layout.size();
            let (w, h) = (w as u32, h as u32);
            let image = glaze_ui::render(&mut ui, &list, w + 2 * edge, h + 2 * edge).unwrap();
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
            glaze_ui::golden::check(&image, &golden.join(format!("{name}.png"))).unwrap();
        }
    }
}

#[cfg(test)]
mod plugin_tests {
    use super::*;
    use crate::widgets::plugin_events::Contribution;

    #[test]
    fn plugin_items_sit_under_their_plugins_name_and_icon() {
        let item = |plugin: &str, id: &str| Contribution { plugin: plugin.into(), name: format!("{plugin} name"), icon: Some("sparkles".into()), id: id.into(), label: id.to_uppercase(), shortcut: None };
        let plugins = [item("hello", "a"), item("hello", "b"), item("other", "c")];
        let items = build_with(&DesktopMenu::default(), ViewState::default(), Sources { plugins: &plugins, ..Sources::default() });
        let submenus: Vec<_> = items.iter().rev().take(2).collect();
        let Entry::Iconic { icon, item } = submenus[1] else { panic!("an icon") };
        assert_eq!(*icon, MenuIcon::Glyph('\u{E734}'));
        let Entry::Submenu { label, items } = &**item else { panic!("a submenu") };
        assert_eq!(label, "hello name");
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[1], Entry::Action { command: Command::Plugin { plugin, id }, .. } if plugin == "hello" && id == "b"));
        assert_eq!(submenus[0].label(), "other name");
        assert!(build(&DesktopMenu::default(), ViewState::default()).iter().all(|i| !matches!(i.plain(), Entry::Submenu { label, .. } if label.ends_with(" name"))));
    }
}
