//! One sortable row for every editable list. Hosts move data only on drop, and map Alt+↑/↓
//! to the same reorder operation. Handles and remove buttons remain keyboard accessible.
use crate::controls::{Align, Behavior, Builder, Font, Glyph, Key, key};
use crate::icons::Icon;
use crate::{Rect, Role};

pub struct Row<'a, A> {
    pub key: Key,
    pub rect: Rect,
    pub label: &'a str,
    pub separator: bool,
    pub icon: Option<Glyph>,
    pub select: A,
    pub drag: A,
    pub remove: A,
}

/// Returns the leading picture rectangle, so hosts can replace an icon with app artwork.
pub fn row<A: Clone>(b: &mut Builder<'_, A>, row: Row<'_, A>) -> Rect {
    let Row {
        key: id,
        rect: r,
        label,
        separator,
        icon,
        select,
        drag,
        remove,
    } = row;
    let handle_key = key((id, "handle"));
    let trash_key = key((id, "remove"));
    let hot = b.input.pointer.is_some_and(|p| r.contains(p))
        || [id, handle_key, trash_key]
            .into_iter()
            .any(|k| b.input.focus == Some(k) || b.pressed(k));
    if hot {
        b.fill(b.list_row_area(r), b.px(6.0), b.p.hover);
    }
    let handle = Rect::new(r.left, r.top, b.px(20.0), r.height());
    b.icon(handle, Icon::Grip, 12.0, b.p.ink3);
    let trash = Rect::new(
        r.right - b.px(28.0),
        r.top + (r.height() - b.px(28.0)) / 2.0,
        b.px(28.0),
        b.px(28.0),
    );
    let picture = Rect::new(
        handle.right + b.px(4.0),
        r.top + (r.height() - b.px(22.0)) / 2.0,
        b.px(22.0),
        b.px(22.0),
    );
    let left = if let Some(icon) = icon {
        b.icon(picture, icon, 16.0, b.p.ink2);
        picture.right + b.px(8.0)
    } else {
        handle.right + b.px(6.0)
    };
    let style = b.font(Font::Body);
    let label_width = if separator {
        b.measure(label, &style)
            .0
            .min((trash.left - left - b.px(34.0)).max(0.0))
    } else {
        (trash.left - left - b.px(6.0)).max(0.0)
    };
    b.text_in(
        Rect::new(left, r.top, label_width, r.height()),
        label,
        &style,
        b.p.ink1,
        Align::Start,
    );
    if separator {
        b.hline(
            left + label_width + b.px(10.0),
            (left + label_width + b.px(42.0)).min(trash.left - b.px(8.0)),
            r.top + r.height() / 2.0,
            b.p.ink3,
        );
    }
    b.hline(r.left, r.right, r.bottom - 1.0, b.p.line);
    b.hit(
        id,
        Rect::new(handle.right, r.top, trash.left - handle.right, r.height()),
        Role::ListItem,
        label,
        select,
        Behavior::Click,
    );
    b.focus_ring(id, b.list_row_area(r), b.px(6.0));
    b.semantics(id, crate::Semantics::Reorder);
    b.hit(
        handle_key,
        handle,
        Role::Button,
        &format!("Reorder {label}; Alt Up or Down"),
        drag,
        Behavior::Drag,
    );
    b.focus_ring(handle_key, handle, b.px(4.0));
    if hot || b.input.focus == Some(trash_key) {
        b.icon(trash, Icon::Delete, 14.0, b.p.ink2);
    }
    b.hit(
        trash_key,
        trash,
        Role::Button,
        &format!("Remove {label}"),
        remove,
        Behavior::Click,
    );
    b.focus_ring(trash_key, trash, b.px(5.0));
    picture
}

/// A drop means insertion, including crossing more than one row. An invalid drop is a no-op.
pub fn reorder<T>(items: &mut Vec<T>, from: usize, to: usize) -> bool {
    if from >= items.len() || to >= items.len() || from == to {
        return false;
    }
    let item = items.remove(from);
    items.insert(to, item);
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn drop_inserts_across_multiple_rows_and_rejects_outside_indices() {
        let mut items = vec!["A", "B", "C", "D"];
        assert!(super::reorder(&mut items, 0, 3));
        assert_eq!(items, ["B", "C", "D", "A"]);
        assert!(super::reorder(&mut items, 3, 1));
        assert_eq!(items, ["B", "A", "C", "D"]);
        assert!(!super::reorder(&mut items, 4, 0));
        assert!(!super::reorder(&mut items, 1, 1));
    }
}
