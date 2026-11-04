//! The desktop menu as data: which items there are (from `desktop_menu` in
//! the config and the desktop's current state), how they are laid out, which
//! one a point or a key selects, and where the menu opens on screen. All pure,
//! so it is tested without a window; `menu_ui` only draws what this describes.

use mypaper_config::desktop_menu::{Builtin, DesktopMenu, Entry};

use crate::desktop_view::{IconSize, NewItem, SortKey};

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

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Separator,
    Action {
        label: String,
        command: Command,
        checked: bool,
        enabled: bool,
    },
    Submenu {
        label: String,
        items: Vec<Item>,
    },
}

impl Item {
    /// Can the pointer or the keyboard land on it?
    pub fn selectable(&self) -> bool {
        match self {
            Self::Separator => false,
            Self::Action { enabled, .. } => *enabled,
            Self::Submenu { .. } => true,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Separator => "",
            Self::Action { label, .. } | Self::Submenu { label, .. } => label,
        }
    }
}

/// What the desktop looks like right now: drives the check marks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewState {
    pub icon_size: Option<IconSize>,
    pub icons_visible: bool,
    /// The sort column and whether it is ascending.
    pub sort: Option<(SortKey, bool)>,
    pub can_paste: bool,
}

fn action(label: &str, command: Command) -> Item {
    Item::Action {
        label: label.into(),
        command,
        checked: false,
        enabled: true,
    }
}

fn checked(label: &str, command: Command, on: bool) -> Item {
    Item::Action {
        label: label.into(),
        command,
        checked: on,
        enabled: true,
    }
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
            Entry::Custom(c) => action(
                &c.label,
                Command::Custom {
                    command: c.command.clone(),
                    args: c.args.clone(),
                },
            ),
            Entry::Builtin(builtin) => builtin_item(*builtin, state),
        })
        .collect()
}

fn builtin_item(builtin: Builtin, state: ViewState) -> Item {
    match builtin {
        Builtin::Separator => Item::Separator,
        Builtin::View => {
            let size = |label, size| {
                checked(
                    label,
                    Command::IconSize(size),
                    state.icon_size == Some(size),
                )
            };
            Item::Submenu {
                label: "View".into(),
                items: vec![
                    size("Large icons", IconSize::Large),
                    size("Medium icons", IconSize::Medium),
                    size("Small icons", IconSize::Small),
                    Item::Separator,
                    checked(
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
                    checked(sort_label(key), Command::SortBy(key), on)
                })
                .collect();
            items.push(Item::Separator);
            let ascending = current.is_none_or(|(_, up)| up);
            items.push(checked(
                "Ascending",
                Command::SortOrder { ascending: true },
                ascending,
            ));
            items.push(checked(
                "Descending",
                Command::SortOrder { ascending: false },
                !ascending,
            ));
            Item::Submenu {
                label: "Sort by".into(),
                items,
            }
        }
        Builtin::Refresh => action("Refresh", Command::Refresh),
        Builtin::New => Item::Submenu {
            label: "New".into(),
            items: vec![
                action("Folder", Command::New(NewItem::Folder)),
                action("Text Document", Command::New(NewItem::TextDocument)),
            ],
        },
        Builtin::Paste => Item::Action {
            label: "Paste".into(),
            command: Command::Paste,
            checked: false,
            enabled: state.can_paste,
        },
        Builtin::Wallpaper => action("Change wallpaper", Command::OpenStudio),
        Builtin::Widgets => action("Edit widgets", Command::EditWidgets),
        Builtin::Display => action("Display settings", Command::Display),
        Builtin::Personalize => action("Personalize", Command::Personalize),
        Builtin::Terminal => action("Open terminal here", Command::Terminal),
        Builtin::More => action("Show more options", Command::MoreOptions),
    }
}

/// Sizes in device pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub row: f32,
    pub separator: f32,
    /// Inner margin of the panel.
    pub pad: f32,
    /// Room for the check mark.
    pub gutter: f32,
    /// Room for a submenu arrow.
    pub arrow: f32,
    /// Between the label and the arrow.
    pub gap: f32,
    pub min_width: f32,
}

impl Metrics {
    /// The standard sizes times `scale` (DPI times the look's scale).
    pub fn scaled(scale: f32) -> Self {
        Self {
            row: 32.0 * scale,
            separator: 9.0 * scale,
            pad: 6.0 * scale,
            gutter: 30.0 * scale,
            arrow: 22.0 * scale,
            gap: 24.0 * scale,
            min_width: 224.0 * scale,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    pub top: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    /// One per item, top to bottom.
    pub rows: Vec<Row>,
}

/// Stacks `items`; `text_width` measures a label in device pixels.
pub fn layout(items: &[Item], m: &Metrics, text_width: impl Fn(&str) -> f32) -> Layout {
    let mut top = m.pad;
    let mut widest = 0f32;
    let rows = items
        .iter()
        .map(|item| {
            let height = if matches!(item, Item::Separator) {
                m.separator
            } else {
                m.row
            };
            let row = Row { top, height };
            top += height;
            if !matches!(item, Item::Separator) {
                widest = widest.max(text_width(item.label()));
            }
            row
        })
        .collect();
    Layout {
        width: (m.pad * 2.0 + m.gutter + widest + m.gap + m.arrow).max(m.min_width),
        height: top + m.pad,
        rows,
    }
}

impl Layout {
    /// The selectable item at panel-relative `y`, if any.
    pub fn item_at(&self, items: &[Item], y: f32) -> Option<usize> {
        let index = self
            .rows
            .iter()
            .position(|r| y >= r.top && y < r.top + r.height)?;
        items[index].selectable().then_some(index)
    }
}

/// The next selectable item after `current` going `delta` (+1 down, -1 up),
/// wrapping around. `None` when nothing can be selected.
pub fn step(items: &[Item], current: Option<usize>, delta: i32) -> Option<usize> {
    let count = items.len() as i32;
    if count == 0 {
        return None;
    }
    let start = current.map_or(if delta > 0 { -1 } else { count }, |i| i as i32);
    (1..=count)
        .map(|n| (start + delta * n).rem_euclid(count) as usize)
        .find(|&i| items[i].selectable())
}

/// A rectangle on screen: left, top, right, bottom.
pub type Edges = (i32, i32, i32, i32);

/// Where the root menu's top-left goes: at the cursor, flipped to open
/// leftward/upward where it would run off `work`, then kept inside it.
pub fn place(size: (i32, i32), anchor: (i32, i32), work: Edges) -> (i32, i32) {
    let (w, h) = size;
    let x = if anchor.0 + w > work.2 {
        anchor.0 - w
    } else {
        anchor.0
    };
    let y = if anchor.1 + h > work.3 {
        anchor.1 - h
    } else {
        anchor.1
    };
    (
        x.clamp(work.0, (work.2 - w).max(work.0)),
        y.clamp(work.1, (work.3 - h).max(work.1)),
    )
}

/// Where a submenu goes: against the right edge of its parent (`parent` is
/// the parent's rect), level with its row; against the left edge instead if
/// the right side has no room; kept inside `work` vertically.
pub fn place_submenu(size: (i32, i32), parent: Edges, row_top: i32, work: Edges) -> (i32, i32) {
    let (w, h) = size;
    let overlap = 4; // a little over the parent, like a native cascade
    let x = if parent.2 - overlap + w <= work.2 {
        parent.2 - overlap
    } else {
        parent.0 + overlap - w
    };
    let y = row_top.min(work.3 - h).max(work.1);
    (x.max(work.0), y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_config::desktop_menu::Custom;

    fn config(items: Vec<Entry>) -> DesktopMenu {
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
                Entry::Builtin(Builtin::Separator),
                Entry::Builtin(Builtin::Refresh),
                Entry::Builtin(Builtin::Separator),
                Entry::Builtin(Builtin::Separator),
                Entry::Custom(Custom {
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
            Item::Action { command: Command::Custom { command, args }, .. }
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
        let marks = |item: &Item| -> Vec<(String, bool)> {
            let Item::Submenu { items, .. } = item else {
                panic!("a submenu")
            };
            items
                .iter()
                .filter_map(|i| match i {
                    Item::Action { label, checked, .. } => Some((label.clone(), *checked)),
                    _ => None,
                })
                .collect()
        };
        let view = marks(&items[0]);
        assert_eq!(
            view,
            [
                ("Large icons".to_owned(), false),
                ("Medium icons".to_owned(), true),
                ("Small icons".to_owned(), false),
                ("Show desktop icons".to_owned(), true),
            ]
        );
        let sort = marks(&items[1]);
        assert!(sort.contains(&("Date modified".to_owned(), true)));
        assert!(sort.contains(&("Name".to_owned(), false)));
        assert!(sort.contains(&("Descending".to_owned(), true)));
        assert!(sort.contains(&("Ascending".to_owned(), false)));

        // Paste is disabled with nothing to paste.
        let paste = items.iter().find(|i| i.label() == "Paste").unwrap();
        assert!(!paste.selectable());
    }

    #[test]
    fn layout_stacks_rows_and_sizes_to_the_widest_label() {
        let m = Metrics::scaled(1.0);
        let items = build(
            &DesktopMenu::default(),
            ViewState {
                can_paste: true,
                ..Default::default()
            },
        );
        let wide = layout(&items, &m, |t| t.len() as f32 * 20.0);
        let narrow = layout(&items, &m, |t| t.len() as f32);
        assert_eq!(narrow.width, m.min_width, "never narrower than the minimum");
        assert!(wide.width > narrow.width);
        assert_eq!(wide.rows.len(), items.len());
        let separators = items
            .iter()
            .filter(|i| matches!(i, Item::Separator))
            .count() as f32;
        let rows = items.len() as f32 - separators;
        assert_eq!(
            wide.height,
            m.pad * 2.0 + rows * m.row + separators * m.separator
        );
        // Rows touch and are in order.
        for pair in wide.rows.windows(2) {
            assert_eq!(pair[0].top + pair[0].height, pair[1].top);
        }
        // Scaling the metrics scales the panel.
        let big = layout(&items, &Metrics::scaled(2.0), |_| 0.0);
        assert_eq!(big.height, wide.height * 2.0);
    }

    #[test]
    fn a_point_selects_the_row_under_it_but_never_a_separator_or_disabled_item() {
        let m = Metrics::scaled(1.0);
        let items = build(&DesktopMenu::default(), ViewState::default()); // paste disabled
        let l = layout(&items, &m, |_| 50.0);
        let center = |i: usize| l.rows[i].top + l.rows[i].height / 2.0;
        assert_eq!(l.item_at(&items, center(0)), Some(0));
        assert_eq!(l.item_at(&items, center(3)), None, "a separator");
        assert_eq!(l.item_at(&items, center(5)), None, "a disabled item");
        assert_eq!(l.item_at(&items, 0.0), None, "the top margin");
        assert_eq!(l.item_at(&items, l.height + 1.0), None);
    }

    #[test]
    fn keys_step_over_separators_and_disabled_items_and_wrap() {
        let items = build(&DesktopMenu::default(), ViewState::default());
        // 0 View, 1 Sort, 2 Refresh, 3 sep, 4 New, 5 Paste (disabled), 6 sep, 7 Wallpaper ...
        assert_eq!(step(&items, None, 1), Some(0), "first");
        assert_eq!(step(&items, None, -1), Some(items.len() - 1), "last");
        assert_eq!(step(&items, Some(2), 1), Some(4));
        assert_eq!(step(&items, Some(4), 1), Some(7));
        assert_eq!(step(&items, Some(4), -1), Some(2));
        assert_eq!(
            step(&items, Some(items.len() - 1), 1),
            Some(0),
            "wraps down"
        );
        assert_eq!(step(&items, Some(0), -1), Some(items.len() - 1), "wraps up");
        assert_eq!(step(&[], None, 1), None);
        assert_eq!(step(&[Item::Separator], None, 1), None);
    }

    #[test]
    fn the_menu_opens_at_the_cursor_and_flips_away_from_screen_edges() {
        let work = (0, 0, 1920, 1040);
        let size = (240, 400);
        assert_eq!(place(size, (100, 100), work), (100, 100));
        assert_eq!(
            place(size, (1800, 100), work),
            (1560, 100),
            "no room right: opens left"
        );
        assert_eq!(
            place(size, (100, 900), work),
            (100, 500),
            "no room below: opens up"
        );
        assert_eq!(place(size, (1800, 900), work), (1560, 500));
        // A menu taller than the screen still starts on screen.
        assert_eq!(place((240, 2000), (100, 500), work), (100, 0));
        // A second monitor to the left (negative coordinates).
        let left = (-2560, 0, 0, 1440);
        assert_eq!(place(size, (-100, 100), left), (-340, 100));
    }

    #[test]
    fn submenus_cascade_right_or_left_level_with_their_row() {
        let work = (0, 0, 1920, 1040);
        let size = (200, 180);
        let parent = (100, 100, 340, 600);
        assert_eq!(place_submenu(size, parent, 140, work), (336, 140));
        // No room on the right: against the parent's left edge.
        let near_right = (1600, 100, 1840, 600);
        assert_eq!(place_submenu(size, near_right, 140, work), (1404, 140));
        // Low rows move up to stay on screen.
        assert_eq!(place_submenu(size, parent, 1000, work), (336, 860));
    }
}
