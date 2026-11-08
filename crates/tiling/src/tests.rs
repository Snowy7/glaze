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
    let mut workspaces = std::collections::HashSet::new();
    for m in &l.monitors {
        assert!(m.workspaces.iter().any(|w| w.id == m.active));
        assert!(m.workspaces.last().unwrap().empty());
    }
    for w in l
        .monitors
        .iter()
        .flat_map(|m| &m.workspaces)
        .chain(&l.detached)
    {
        assert!(workspaces.insert(w.id));
        assert!(w.view.is_finite());
        assert_eq!(w.empty(), w.focused.is_none());
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
            w.focused
                .is_none_or(|id| w.column(id).is_some() || w.floating.iter().any(|f| f.id == id))
        );
        if let Some(ci) = w.focused.and_then(|id| w.column(id)) {
            assert_eq!(
                Some(w.columns[ci].tiles[w.columns[ci].active].id),
                w.focused
            );
        }
    }
    for t in l.targets() {
        assert!(ids.contains(&t.window) && workspaces.contains(&t.workspace));
        assert!(t.rect.x.is_finite() && t.rect.y.is_finite() && t.rect.w > 0.0 && t.rect.h > 0.0);
    }
}
#[test]
fn defaults_and_bindings_are_valid() {
    Settings::default().validate().unwrap();
}
#[test]
fn floating_windows_follow_the_work_area_and_dpi_through_zero_outputs() {
    let mut l = layout();
    let b = Rect {
        x: 1200.0,
        ..area()
    };
    l.add_monitor("b".into(), b, b, 2.0);
    l.focused_monitor = Some("b".into());
    let original = Rect {
        x: 1500.0,
        y: 200.0,
        w: 600.0,
        h: 400.0,
    };
    l.insert(
        WindowId(1),
        Constraints::default(),
        None,
        Some(original),
        true,
    );
    l.remove_monitor("b");
    assert_eq!(
        l.targets()[0].rect,
        Rect {
            x: 300.0,
            y: 200.0,
            w: 300.0,
            h: 200.0
        }
    );
    l.remove_monitor("a");
    assert!(l.targets().is_empty());
    l.add_monitor("b".into(), b, b, 2.0);
    assert_eq!(l.targets()[0].rect, original);
    let small = Rect {
        x: -800.0,
        w: 800.0,
        h: 600.0,
        ..area()
    };
    l.add_monitor("b".into(), small, small, 1.0);
    let r = l.targets()[0].rect;
    assert!(small.contains(r.x + r.w / 2.0, r.y + r.h / 2.0));
    assert_eq!((r.w, r.h), (300.0, 200.0));
    check(&l);
}
#[test]
fn floating_toggle_remembers_geometry_and_constraints() {
    let mut l = layout();
    let original = Rect {
        x: 200.0,
        y: 150.0,
        w: 400.0,
        h: 300.0,
    };
    let constraints = Constraints {
        min_w: 300.0,
        min_h: 200.0,
        ..Constraints::default()
    };
    l.insert(WindowId(1), constraints, None, Some(original), true);
    l.command(Command::Floating);
    assert!(!l.is_floating(WindowId(1)));
    l.command(Command::Floating);
    assert_eq!(l.targets()[0].rect, original);
    let (mi, wi) = l.locate(WindowId(1)).unwrap();
    assert_eq!(
        l.monitors[mi].workspaces[wi].floating[0].constraints,
        constraints
    );
    l.command(Command::Floating);
    l.remove(WindowId(1));
    assert!(!l.floating_memory.contains_key(&WindowId(1)));
    check(&l);
}
#[test]
fn fullscreen_float_returns_to_its_geometry_even_after_maximizing() {
    let mut l = layout();
    let original = Rect {
        x: 200.0,
        y: 150.0,
        w: 400.0,
        h: 300.0,
    };
    let constraints = Constraints {
        min_w: 300.0,
        ..Constraints::default()
    };
    l.insert(WindowId(1), constraints, None, Some(original), true);
    l.command(Command::Fullscreen);
    assert!(!l.is_floating(WindowId(1)));
    assert_eq!(l.targets()[0].rect, area());
    l.command(Command::Maximize);
    l.command(Command::Fullscreen);
    l.command(Command::Fullscreen);
    assert!(l.is_floating(WindowId(1)));
    assert_eq!(l.targets()[0].rect, original);
    let saved = l.suspend(WindowId(1)).unwrap();
    l.resume(saved, true);
    let (mi, wi) = l.locate(WindowId(1)).unwrap();
    assert_eq!(
        l.monitors[mi].workspaces[wi].floating[0].constraints,
        constraints
    );
    check(&l);
}
#[test]
fn game_bar_screenshot_and_recording_timer_remain_reserved() {
    for key in [0x2C, 0x54] {
        let mut settings = Settings::default();
        settings.keybindings = vec![Binding {
            key,
            ctrl: false,
            shift: false,
            alt: true,
            command: Command::Overview,
        }];
        assert!(settings.validate().is_err());
    }
}
#[test]
fn full_width_retains_gaps_and_restores_the_previous_width() {
    let mut l = layout();
    open(&mut l, 1);
    let before = l.targets()[0].rect;
    l.command(Command::FullWidth);
    assert_eq!(
        l.targets()[0].rect,
        Rect {
            x: 16.0,
            y: 16.0,
            w: 1168.0,
            h: 768.0
        }
    );
    l.command(Command::FullWidth);
    assert_eq!(l.targets()[0].rect, before);
    l.command(Command::Maximize);
    assert_eq!(l.targets()[0].rect, area());
    l.command(Command::Maximize);
    assert_eq!(l.targets()[0].rect.w, before.w);
}
#[test]
fn on_overflow_keeps_two_half_width_columns_together() {
    let mut l = layout();
    l.settings.center_focused_column = Center::OnOverflow;
    for id in 1..=3 {
        open(&mut l, id);
    }
    assert_eq!(
        l.targets()
            .iter()
            .find(|t| t.window == WindowId(3))
            .unwrap()
            .rect
            .x,
        608.0
    );
}
#[test]
fn on_overflow_centers_when_the_adjacent_pair_cannot_fit() {
    let mut l = layout();
    l.settings.center_focused_column = Center::OnOverflow;
    l.settings.default_width = 2.0 / 3.0;
    open(&mut l, 1);
    open(&mut l, 2);
    let r = l
        .targets()
        .into_iter()
        .find(|t| t.window == WindowId(2))
        .unwrap()
        .rect;
    assert!((r.x - (area().w - r.w) / 2.0).abs() < 0.001);
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
fn closing_a_consumed_temporary_window_restores_the_correct_tab() {
    let mut l = layout();
    open(&mut l, 1);
    open(&mut l, 2);
    l.command(Command::Consume);
    l.focus(WindowId(1));
    open(&mut l, 3);
    l.command(Command::Consume);
    l.command(Command::Tabbed);
    l.remove(WindowId(3));
    assert_eq!(l.focused(), Some(WindowId(1)));
    assert!(
        l.targets()
            .iter()
            .any(|t| t.window == WindowId(1) && t.visible)
    );
    check(&l);
}
#[test]
fn closing_a_preceding_tab_keeps_focus_with_and_without_outputs() {
    for detached in [false, true] {
        let mut l = layout();
        open(&mut l, 1);
        open(&mut l, 2);
        l.command(Command::Consume);
        open(&mut l, 3);
        l.command(Command::Consume);
        l.focus(WindowId(2));
        l.command(Command::Tabbed);
        if detached {
            l.remove_monitor("a");
        }
        l.remove(WindowId(1));
        check(&l);
        let w = if detached {
            &l.detached[0]
        } else {
            &l.monitors[0].workspaces[0]
        };
        assert_eq!(w.focused, Some(WindowId(2)));
        assert_eq!(w.columns[0].tiles[w.columns[0].active].id, WindowId(2));
    }
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
fn layer_switch_returns_to_the_last_keyboard_focused_tile() {
    let mut l = layout();
    open(&mut l, 1);
    open(&mut l, 2);
    l.command(Command::Focus(Direction::Left));
    l.insert(
        WindowId(3),
        Constraints::default(),
        None,
        Some(area()),
        false,
    );
    l.command(Command::FocusLayer);
    assert_eq!(l.focused(), Some(WindowId(3)));
    l.command(Command::FocusLayer);
    assert_eq!(l.focused(), Some(WindowId(1)));
    check(&l);
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
    l.command(Command::FullWidth);
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
fn a_stack_members_minimum_keeps_the_focused_column_visible() {
    let mut l = layout();
    open(&mut l, 1);
    open(&mut l, 2);
    l.command(Command::Consume);
    l.learn_minimum(WindowId(1), 1200.0);
    assert!(
        l.targets()
            .iter()
            .any(|t| t.window == WindowId(2) && t.visible)
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
fn closing_a_float_without_any_outputs_keeps_the_remaining_float_focused() {
    let mut l = layout();
    for id in 1..=2 {
        l.insert(
            WindowId(id),
            Constraints::default(),
            None,
            Some(area()),
            true,
        );
    }
    l.remove_monitor("a");
    l.remove(WindowId(2));
    assert_eq!(l.detached[0].focused, Some(WindowId(1)));
    check(&l);
}
#[test]
fn closing_the_last_detached_float_returns_to_the_selected_tab() {
    let mut l = layout();
    open(&mut l, 1);
    open(&mut l, 2);
    l.command(Command::Consume);
    l.insert(
        WindowId(3),
        Constraints::default(),
        None,
        Some(area()),
        true,
    );
    l.remove_monitor("a");
    l.remove(WindowId(3));
    assert_eq!(l.detached[0].focused, Some(WindowId(2)));
    check(&l);
}
#[test]
fn a_smaller_output_refits_the_focused_column() {
    let mut l = layout();
    for id in 1..=4 {
        open(&mut l, id);
    }
    let smaller = Rect {
        w: 800.0,
        h: 600.0,
        ..area()
    };
    l.add_monitor("a".into(), smaller, smaller, 1.5);
    assert!(
        l.targets()
            .iter()
            .find(|t| t.window == WindowId(4))
            .unwrap()
            .visible
    );
    assert_eq!(l.focused(), Some(WindowId(4)));
}
#[test]
fn unplug_and_reconnect_keep_the_active_workspace_and_focus() {
    let mut l = layout();
    let smaller = Rect {
        w: 800.0,
        h: 600.0,
        ..area()
    };
    l.add_monitor("b".into(), smaller, smaller, 1.5);
    for id in 1..=4 {
        open(&mut l, id);
    }
    l.remove_monitor("a");
    assert_eq!(l.focused(), Some(WindowId(4)));
    assert!(
        l.targets()
            .iter()
            .find(|t| t.window == WindowId(4))
            .unwrap()
            .visible
    );
    l.add_monitor("a".into(), area(), area(), 1.0);
    assert_eq!(l.focused(), Some(WindowId(4)));
    assert_eq!(l.focused_monitor.as_deref(), Some("a"));
    check(&l);
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
fn a_background_fullscreen_return_preserves_the_focused_columns_position() {
    let mut l = layout();
    open(&mut l, 1);
    open(&mut l, 2);
    l.command(Command::Center);
    let before = l
        .targets()
        .into_iter()
        .find(|t| t.window == WindowId(2))
        .unwrap()
        .rect;
    let saved = l.suspend(WindowId(1)).unwrap();
    l.resume(saved, false);
    assert_eq!(
        l.targets()
            .into_iter()
            .find(|t| t.window == WindowId(2))
            .unwrap()
            .rect,
        before
    );
}
#[test]
fn native_fullscreen_returns_to_its_original_stack_and_tab() {
    let mut l = layout();
    open(&mut l, 1);
    open(&mut l, 2);
    l.command(Command::Consume);
    l.command(Command::Tabbed);
    let saved = l.suspend(WindowId(2)).unwrap();
    l.resume(saved, true);
    let w = &l.monitors[0].workspaces[0];
    assert_eq!(w.columns.len(), 1);
    assert_eq!(w.columns[0].tiles.len(), 2);
    assert!(w.columns[0].tabbed);
    assert_eq!(w.columns[0].tiles[w.columns[0].active].id, WindowId(2));
    check(&l);
}
#[test]
fn keyboard_output_and_workspace_moves_carry_the_entire_column() {
    let mut l = layout();
    let second = Rect {
        x: 2000.0,
        ..area()
    };
    l.add_monitor("b".into(), second, second, 1.0);
    open(&mut l, 1);
    open(&mut l, 2);
    l.command(Command::Consume);
    l.command(Command::Tabbed);
    l.command(Command::CycleWidth);
    l.command(Command::MoveMonitor(Direction::Right));
    let (mi, wi) = l.locate(WindowId(2)).unwrap();
    assert_eq!(l.monitors[mi].id, "b");
    assert_eq!(l.locate(WindowId(1)), Some((mi, wi)));
    assert_eq!(l.monitors[mi].workspaces[wi].columns[0].width, 2.0 / 3.0);
    assert!(l.monitors[mi].workspaces[wi].columns[0].tabbed);
    let workspace = l.monitors[mi].workspaces[wi].id;
    l.command(Command::MoveWorkspace(Direction::Down));
    let (mi, wi) = l.locate(WindowId(2)).unwrap();
    assert_eq!(l.locate(WindowId(1)), Some((mi, wi)));
    assert_ne!(l.monitors[mi].workspaces[wi].id, workspace);
    check(&l);
}
#[test]
fn fullscreen_expels_a_stacked_window_without_rejoining_on_exit() {
    let mut l = layout();
    open(&mut l, 1);
    open(&mut l, 2);
    l.command(Command::Consume);
    l.command(Command::Fullscreen);
    let targets = l.targets();
    assert_eq!(targets.iter().filter(|t| t.visible).count(), 1);
    assert!(targets.iter().any(|t| t.window == WindowId(2) && t.visible));
    let (mi, wi) = l.locate(WindowId(2)).unwrap();
    assert_eq!(l.monitors[mi].workspaces[wi].columns.len(), 2);
    l.command(Command::Fullscreen);
    assert!(
        l.targets()
            .iter()
            .any(|t| t.window == WindowId(2) && t.visible)
    );
    assert!(
        l.monitors[mi].workspaces[wi]
            .columns
            .iter()
            .all(|c| c.mode == Mode::Normal)
    );
    assert_eq!(l.monitors[mi].workspaces[wi].columns.len(), 2);
}
#[test]
fn maximized_tabbed_columns_show_one_window_to_the_work_area_edges() {
    let mut l = layout();
    open(&mut l, 1);
    open(&mut l, 2);
    l.command(Command::Consume);
    l.command(Command::Tabbed);
    l.command(Command::Maximize);
    let targets = l.targets();
    let active = targets.iter().find(|t| t.window == WindowId(2)).unwrap();
    assert!(active.visible);
    assert_eq!(active.rect, area());
    assert!(
        !targets
            .iter()
            .find(|t| t.window == WindowId(1))
            .unwrap()
            .visible
    );
    l.command(Command::Maximize);
    let (mi, wi) = l.locate(WindowId(2)).unwrap();
    assert_eq!(l.monitors[mi].workspaces[wi].columns.len(), 1);
    l.command(Command::Tabbed);
    l.set_maximized(WindowId(2), true);
    assert_eq!(l.monitors[mi].workspaces[wi].columns.len(), 2);
    check(&l);
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
    fn random_operations_preserve_invariants(ops in prop::collection::vec((0u8..36,0u64..24,1u16..2000),1..200)){
        let mut l=layout();
        for (op,id,size) in ops{
            let id=WindowId(id);
            match op{
                0|1=>{let old=l.targets();l.insert(id,Constraints{min_w:f64::from(size),min_h:f64::from(size%400),max_w:Some(f64::from(size)/2.0),max_h:Some(f64::from(size)/3.0)},None,None,op==0);for t in old{if let Some(new)=l.targets().iter().find(|n|n.window==t.window){prop_assert_eq!((new.rect.w,new.rect.h),(t.rect.w,t.rect.h));}}},
                2=>{let focused=l.focused();let before=l.targets().into_iter().find(|t|Some(t.window)==focused);l.remove(id);if focused!=Some(id){if let Some(before)=before{let after=l.targets().into_iter().find(|t|Some(t.window)==focused).unwrap();prop_assert!((before.rect.x-after.rect.x).abs()<0.00001 );}}},
                3=>{l.focus(id);},4=>{l.command(Command::Focus(Direction::Left));},5=>{l.command(Command::Focus(Direction::Right));},6=>{l.command(Command::Consume);},7=>{l.command(Command::Expel);},8=>{l.command(Command::Tabbed);},9=>{l.command(Command::Workspace(Direction::Down));},10=>{l.command(Command::Workspace(Direction::Up));},11=>{l.remove_monitor("a");},12=>{l.add_monitor("a".into(),area(),area(),f64::from(size)/400.0);},13=>{l.command(Command::Maximize);},14=>{l.command(Command::Floating);},15=>{l.command(Command::Move(Direction::Right));},
                16=>{l.command(Command::Fullscreen);},17=>{l.command(Command::FullWidth);},18=>{l.command(Command::FocusLayer);},
                19=>{l.command(Command::CycleWidth);},20=>{l.command(Command::CycleHeight);},21=>{l.learn_minimum(id,f64::from(size));},
                22=>{l.resize_column(id,f64::from(size));},23=>{if let Some(saved)=l.suspend(id){l.resume(saved,false);}},
                24=>{l.float_at(id,Rect{x:50.0,y:60.0,w:300.0,h:200.0});},25=>{l.drop_at(id,f64::from(size%1200),f64::from(size%800));},
                26=>{l.drop_on_workspace(id,"a",usize::from(size%3),size%2==0);},
                27=>{l.add_monitor("b".into(),area(),area(),1.5);},28=>{l.remove_monitor("b");},
                29=>{l.command(Command::FocusMonitor(Direction::Right));},
                30|31=>{l.insert(id,Constraints::default(),Some(WindowId((id.0+1)%24)),(op==31).then_some(area()),false);},
                32=>{l.command(Command::MoveWorkspace(Direction::Down));},33=>{l.command(Command::MoveMonitor(Direction::Right));},
                34=>{l.command(Command::Focus(Direction::Up));},_=>{l.command(Command::Focus(Direction::Down));}
            }
            check(&l);
        }
    }
}
