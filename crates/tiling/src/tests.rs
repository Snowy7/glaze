//! Invariants and named regressions for changes that used to disturb the focused column.
use super::*;
use proptest::prelude::*;

fn area() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        w: 1200.0,
        h: 800.0,
    }
}
fn layout() -> Layout {
    let mut l = Layout::new(Settings::default());
    l.add_monitor("a".into(), area(), area(), 1.0);
    l
}
fn open(l: &mut Layout, id: u64) {
    l.insert(WindowId(id), Constraints::default(), None, None, true);
}
fn check(l: &Layout) {
    let mut ids = std::collections::HashSet::new();
    for m in &l.monitors {
        assert!(m.workspaces.iter().any(|w| w.id == m.active));
        assert!(m.workspaces.last().unwrap().empty());
        for w in &m.workspaces {
            assert!(w.view.is_finite());
            for c in &w.columns {
                assert!(!c.tiles.is_empty());
                assert!(c.active < c.tiles.len());
                for t in &c.tiles {
                    assert!(ids.insert(t.id));
                }
            }
            for f in &w.floating {
                assert!(ids.insert(f.id));
            }
            assert!(
                w.focused.is_none_or(
                    |id| w.column(id).is_some() || w.floating.iter().any(|f| f.id == id)
                )
            );
        }
    }
    for t in l.targets() {
        assert!(t.rect.x.is_finite() && t.rect.y.is_finite() && t.rect.w > 0.0 && t.rect.h > 0.0);
    }
}
#[test]
fn defaults_and_bindings_are_valid() {
    Settings::default().validate().unwrap();
}
#[test]
fn closing_just_opened_column_restores_view() {
    let mut l = layout();
    open(&mut l, 1);
    let before = l.targets();
    open(&mut l, 2);
    l.remove(WindowId(2));
    assert_eq!(l.focused(), Some(WindowId(1)));
    assert_eq!(l.targets(), before);
}
#[test]
fn closing_left_column_preserves_focused_screen_position() {
    let mut l = layout();
    for id in 1..5 {
        open(&mut l, id);
    }
    let before = l
        .targets()
        .into_iter()
        .find(|t| t.window == WindowId(4))
        .unwrap()
        .rect;
    l.remove(WindowId(1));
    assert_eq!(
        l.targets()
            .into_iter()
            .find(|t| t.window == WindowId(4))
            .unwrap()
            .rect,
        before
    );
}
#[test]
fn closing_a_left_column_keeps_the_temporary_views_anchor() {
    let mut l = layout();
    for id in 1..=3 {
        open(&mut l, id);
    }
    let before = l
        .targets()
        .into_iter()
        .find(|t| t.window == WindowId(3))
        .unwrap()
        .rect;
    open(&mut l, 4);
    l.remove(WindowId(1));
    l.remove(WindowId(4));
    assert_eq!(l.focused(), Some(WindowId(3)));
    assert_eq!(
        l.targets()
            .into_iter()
            .find(|t| t.window == WindowId(3))
            .unwrap()
            .rect,
        before
    );
}
#[test]
fn closing_a_float_returns_focus_to_the_tiled_layer() {
    let mut l = layout();
    open(&mut l, 1);
    l.insert(
        WindowId(2),
        Constraints::default(),
        None,
        Some(area()),
        true,
    );
    l.focus(WindowId(1));
    l.focus(WindowId(2));
    l.remove(WindowId(2));
    assert_eq!(l.focused(), Some(WindowId(1)));
}
#[test]
fn closing_a_temporary_window_after_its_anchor_closed_still_refocuses() {
    let mut l = layout();
    for id in 1..=3 {
        open(&mut l, id);
    }
    l.remove(WindowId(2));
    l.remove(WindowId(3));
    assert_eq!(l.focused(), Some(WindowId(1)));
}
#[test]
fn explicit_focus_forgets_temporary_view() {
    let mut l = layout();
    for id in 1..4 {
        open(&mut l, id);
    }
    l.focus(WindowId(1));
    l.focus(WindowId(3));
    l.remove(WindowId(3));
    assert_eq!(l.focused(), Some(WindowId(2)));
}
#[test]
fn minimum_wins_over_maximum() {
    let mut l = layout();
    l.insert(
        WindowId(1),
        Constraints {
            min_w: 900.0,
            max_w: Some(200.0),
            ..Constraints::default()
        },
        None,
        None,
        true,
    );
    assert_eq!(l.targets()[0].rect.w, 900.0);
}
#[test]
fn a_left_clients_size_request_does_not_move_the_focused_window() {
    let mut l = layout();
    for id in 1..=3 {
        open(&mut l, id);
    }
    let before = l
        .targets()
        .into_iter()
        .find(|t| t.window == WindowId(3))
        .unwrap()
        .rect;
    l.learn_minimum(WindowId(1), 900.0);
    assert_eq!(
        l.targets()
            .into_iter()
            .find(|t| t.window == WindowId(3))
            .unwrap()
            .rect,
        before
    );
    l.resize_column(WindowId(2), 300.0);
    assert_eq!(
        l.targets()
            .into_iter()
            .find(|t| t.window == WindowId(3))
            .unwrap()
            .rect,
        before
    );
}
#[test]
fn portrait_rotates_both_layout_axes() {
    let mut l = Layout::new(Settings::default());
    let r = Rect {
        w: 800.0,
        h: 1200.0,
        ..area()
    };
    l.add_monitor("portrait".into(), r, r, 1.0);
    open(&mut l, 1);
    let r = l.targets()[0].rect;
    assert_eq!(r.w, 768.0);
    assert_eq!(r.h, 576.0);
}
#[test]
fn reconnect_returns_workspaces_unless_opened_on_survivor() {
    let mut l = layout();
    l.add_monitor("b".into(), area(), area(), 1.5);
    open(&mut l, 1);
    l.remove_monitor("a");
    assert_eq!(l.locate(WindowId(1)).unwrap().0, 0);
    l.add_monitor("a".into(), area(), area(), 1.0);
    assert_eq!(l.monitors[l.locate(WindowId(1)).unwrap().0].id, "a");
    l.remove_monitor("a");
    l.focus(WindowId(1));
    open(&mut l, 2);
    l.add_monitor("a".into(), area(), area(), 1.0);
    assert_eq!(l.monitors[l.locate(WindowId(1)).unwrap().0].id, "b");
}
#[test]
fn zero_monitors_preserves_windows() {
    let mut l = layout();
    open(&mut l, 1);
    l.remove_monitor("a");
    assert!(l.targets().is_empty());
    assert_eq!(l.detached[0].columns[0].tiles[0].id, WindowId(1));
    l.add_monitor("a".into(), area(), area(), 1.0);
    assert!(l.locate(WindowId(1)).is_some());
}
#[test]
fn owner_loops_do_not_recurse() {
    let mut l = layout();
    l.insert(
        WindowId(1),
        Constraints::default(),
        Some(WindowId(1)),
        None,
        true,
    );
    l.insert(
        WindowId(2),
        Constraints::default(),
        Some(WindowId(1)),
        Some(area()),
        true,
    );
    check(&l);
}
#[test]
fn tabbed_columns_park_inactive_tabs() {
    let mut l = layout();
    open(&mut l, 1);
    open(&mut l, 2);
    l.command(Command::Consume);
    l.command(Command::Tabbed);
    let t = l.targets();
    assert_eq!(t.iter().filter(|t| t.visible).count(), 1);
    assert!(t.iter().find(|t| t.window == WindowId(2)).unwrap().visible);
}
#[test]
fn fullscreen_returns_to_original_column_and_width() {
    let mut l = layout();
    for id in 1..=3 {
        open(&mut l, id);
    }
    l.focus(WindowId(2));
    l.command(Command::CycleWidth);
    let saved = l.suspend(WindowId(2)).unwrap();
    l.resume(saved, true);
    let w = &l.monitors[0].workspaces[0];
    assert_eq!(w.columns[1].tiles[0].id, WindowId(2));
    assert_eq!(w.columns[1].width, 2.0 / 3.0);
}
#[test]
fn drop_at_column_center_joins_and_edge_inserts() {
    let mut l = layout();
    open(&mut l, 1);
    open(&mut l, 2);
    l.focus(WindowId(1));
    let r = l
        .targets()
        .into_iter()
        .find(|t| t.window == WindowId(1))
        .unwrap()
        .rect;
    l.drop_at(WindowId(2), r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert_eq!(l.monitors[0].workspaces[0].columns.len(), 1);
    assert_eq!(l.monitors[0].workspaces[0].columns[0].tiles.len(), 2);
    check(&l);
}
#[test]
fn stacked_minima_are_allocated_before_weighted_heights() {
    let mut l = layout();
    l.insert(
        WindowId(1),
        Constraints {
            min_h: 600.0,
            ..Constraints::default()
        },
        None,
        None,
        true,
    );
    open(&mut l, 2);
    l.command(Command::Consume);
    let t = l.targets();
    assert_eq!(
        t.iter().find(|t| t.window == WindowId(1)).unwrap().rect.h,
        600.0
    );
    assert_eq!(
        t.iter().find(|t| t.window == WindowId(2)).unwrap().rect.h,
        152.0
    );
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn random_operations_preserve_invariants(ops in prop::collection::vec((0u8..16,0u64..24,1u16..2000),1..200)){
        let mut l=layout();
        for (op,id,size) in ops{
            let id=WindowId(id);
            match op{
                0|1=>{let old=l.targets();l.insert(id,Constraints{min_w:f64::from(size),max_w:Some(f64::from(size)/2.0),..Constraints::default()},None,None,op==0);for t in old{if let Some(new)=l.targets().iter().find(|n|n.window==t.window){prop_assert_eq!((new.rect.w,new.rect.h),(t.rect.w,t.rect.h));}}},
                2=>{let focused=l.focused();let before=l.targets().into_iter().find(|t|Some(t.window)==focused);l.remove(id);if focused!=Some(id){if let Some(before)=before{let after=l.targets().into_iter().find(|t|Some(t.window)==focused).unwrap();prop_assert!((before.rect.x-after.rect.x).abs()<0.00001 );}}},
                3=>{l.focus(id);},4=>{l.command(Command::Focus(Direction::Left));},5=>{l.command(Command::Focus(Direction::Right));},6=>{l.command(Command::Consume);},7=>{l.command(Command::Expel);},8=>{l.command(Command::Tabbed);},9=>{l.command(Command::Workspace(Direction::Down));},10=>{l.command(Command::Workspace(Direction::Up));},11=>{l.remove_monitor("a");},12=>{l.add_monitor("a".into(),area(),area(),f64::from(size)/400.0);},13=>{l.command(Command::Maximize);},14=>{l.command(Command::Floating);},_=>{l.command(Command::Move(Direction::Right));}
            }
            check(&l);
        }
    }
}
