//! UI Automation through AccessKit. Every surface retains its latest hit snapshot; startup
//! surfaces can defer the adapter itself until WM_GETOBJECT. Neither constructor builds a tree
//! before a client asks, and updates do no UIA work while the adapter is inactive or deferred.

use std::rc::Rc;
use std::sync::Arc;

use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Role, Toggled, TreeId,
    TreeInfo, TreeUpdate,
};
use accesskit_windows::{Adapter, QueuedEvents};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::scene::{Hit, HitId};

/// Posted to a surface when assistive technology acts on one of its hit items:
/// `wParam` is 0 for click (invoke) and 1 for focus, `lParam` the hit id.
pub(crate) const WM_A11Y_ACTION: u32 = WM_APP + 0x3a11;

const ROOT: NodeId = NodeId(0);

fn node_id(hit: HitId) -> NodeId {
    NodeId(u64::from(hit) + 1)
}

/// What the tree is built from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Snapshot {
    pub role: Role,
    pub name: Arc<str>,
    pub hits: Vec<Hit>,
    pub focus: Option<HitId>,
}

impl Snapshot {
    /// AccessKit panics (and `panic = "abort"` kills the process) on a duplicate child or a
    /// focus outside the tree, so the first hit per id wins and a stale focus falls back to
    /// the root. Views should still give every hit its own id.
    fn tree(&self) -> TreeUpdate {
        let mut seen = std::collections::HashSet::new();
        let hits: Vec<&Hit> = self.hits.iter().filter(|h| seen.insert(h.id)).collect();
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if hits.len() != self.hits.len() && !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            log::warn!(
                "a11y: {} hits share an id in {:?}; only the first of each is exposed",
                self.hits.len() - hits.len(),
                &*self.name
            );
        }
        let mut root = Node::new(self.role);
        root.set_label(&*self.name);
        root.set_children(hits.iter().map(|h| node_id(h.id)).collect::<Vec<_>>());
        let mut nodes = vec![(ROOT, root)];
        nodes.extend(hits.iter().map(|hit| {
            let mut node = Node::new(hit.role);
            node.set_label(&*hit.label);
            node.set_bounds(accesskit::Rect {
                x0: f64::from(hit.rect.left),
                y0: f64::from(hit.rect.top),
                x1: f64::from(hit.rect.right),
                y1: f64::from(hit.rect.bottom),
            });
            if hit.enabled {
                node.add_action(Action::Click);
                node.add_action(Action::Focus);
            } else {
                node.set_disabled();
            }
            if let Some(on) = hit.checked {
                node.set_toggled(if on { Toggled::True } else { Toggled::False });
            }
            (node_id(hit.id), node)
        }));
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(ROOT)),
            tree_id: TreeId::ROOT,
            focus: self
                .focus
                .filter(|focus| seen.contains(focus))
                .map_or(ROOT, node_id),
        }
    }
}

/// A surface's accessibility: the adapter and the latest snapshot.
pub(crate) struct A11y {
    adapter: Option<Adapter>,
    hwnd: HWND,
    snapshot: Rc<Snapshot>,
}

impl A11y {
    pub(crate) fn new(hwnd: HWND, role: Role, name: Arc<str>) -> Self {
        let mut a11y = Self::deferred(hwnd, role, name);
        a11y.adapter = Some(Adapter::new(
            hwnd,
            false,
            Actions {
                hwnd: hwnd.0 as isize,
            },
        ));
        a11y
    }

    /// Keep the initial snapshot without loading UI Automation before a surface's first paint.
    pub(crate) fn deferred(hwnd: HWND, role: Role, name: Arc<str>) -> Self {
        Self {
            adapter: None,
            hwnd,
            snapshot: Rc::new(Snapshot {
                role,
                name,
                hits: Vec::new(),
                focus: None,
            }),
        }
    }

    /// New hit items or focus; the tree follows only once a client has asked for it. Raise the
    /// returned events after letting go of any borrow.
    pub(crate) fn update(&mut self, hits: &[Hit], focus: Option<HitId>) -> Option<QueuedEvents> {
        if self.snapshot.hits == hits && self.snapshot.focus == focus {
            return None;
        }
        let next = Rc::new(Snapshot {
            hits: hits.to_vec(),
            focus,
            ..(*self.snapshot).clone()
        });
        self.snapshot = next.clone();
        self.adapter.as_mut()?.update_if_active(|| next.tree())
    }

    pub(crate) fn focus_changed(&mut self, focused: bool) -> Option<QueuedEvents> {
        self.adapter.as_mut()?.update_window_focus_state(focused)
    }

    /// Answers `WM_GETOBJECT`, building the tree on the first request. The result must be
    /// converted to an `LRESULT` after letting go of any borrow (it may call back into us).
    pub(crate) fn get_object(
        &mut self,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<impl Into<LRESULT> + use<>> {
        let mut initial = Initial(self.snapshot.clone());
        let hwnd = self.hwnd;
        self.adapter
            .get_or_insert_with(|| {
                Adapter::new(
                    hwnd,
                    false,
                    Actions {
                        hwnd: hwnd.0 as isize,
                    },
                )
            })
            .handle_wm_getobject(wparam, lparam, &mut initial)
    }
}

/// Builds the first tree on demand.
struct Initial(Rc<Snapshot>);

impl ActivationHandler for Initial {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(self.0.tree())
    }
}

/// Forwards assistive technology's requests to the surface's thread as [`WM_A11Y_ACTION`].
struct Actions {
    hwnd: isize,
}

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        let kind = match request.action {
            Action::Click => 0,
            Action::Focus => 1,
            _ => return,
        };
        let Some(hit) = request.target_node.0.checked_sub(1) else {
            return;
        };
        // SAFETY: posting to a window handle is always safe; failure means it is gone.
        let _ = unsafe {
            PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                WM_A11Y_ACTION,
                WPARAM(kind),
                LPARAM(hit as isize),
            )
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;

    #[test]
    fn deferred_adapter_keeps_the_latest_hits_before_activation() {
        let mut a11y = A11y::deferred(HWND::default(), Role::Toolbar, "bar".into());
        let mut hits = vec![Hit {
            id: 7,
            rect: Rect::new(0.0, 0.0, 40.0, 24.0),
            role: Role::Button,
            label: "before".into(),
            enabled: true,
            checked: None,
        }];
        assert!(a11y.update(&hits, None).is_none());
        hits[0].label = "latest".into();
        assert!(a11y.update(&hits, Some(7)).is_none());
        assert!(
            a11y.adapter.is_none(),
            "updates must not initialize UI Automation"
        );
        let tree = Initial(a11y.snapshot.clone())
            .request_initial_tree()
            .unwrap();
        assert_eq!(tree.nodes[1].1.label(), Some("latest"));
        assert_eq!(tree.focus, node_id(7));
    }

    #[test]
    fn hits_become_nodes_with_their_state_and_focus() {
        let hit = |id, enabled, checked| Hit {
            id,
            rect: Rect::new(0.0, id as f32 * 10.0, 50.0, 10.0),
            role: Role::MenuItem,
            label: format!("item {id}").into(),
            enabled,
            checked,
        };
        let snapshot = Snapshot {
            role: Role::Menu,
            name: "menu".into(),
            hits: vec![hit(0, true, Some(true)), hit(2, false, None)],
            focus: Some(2),
        };
        let tree = snapshot.tree();
        assert_eq!(tree.focus, NodeId(3));
        assert_eq!(tree.nodes.len(), 3);
        assert_eq!(tree.nodes[0].1.children(), [NodeId(1), NodeId(3)]);
        let (_, first) = &tree.nodes[1];
        assert_eq!(first.toggled(), Some(Toggled::True));
        assert!(!first.is_disabled());
        assert!(tree.nodes[2].1.is_disabled());
        assert_eq!(tree.nodes[2].1.label(), Some("item 2"));
    }

    /// AccessKit aborts the process on a duplicate child or a focus outside the tree (the
    /// calendar editor's crash): the first hit per id wins and a stale focus is the root.
    #[test]
    fn duplicate_hits_and_stale_focus_still_make_a_valid_tree() {
        let hit = |id, label: &str| Hit {
            id,
            rect: Rect::new(0.0, 0.0, 50.0, 10.0),
            role: Role::Button,
            label: label.into(),
            enabled: true,
            checked: None,
        };
        let mut snapshot = Snapshot {
            role: Role::Pane,
            name: "calendar".into(),
            hits: vec![hit(6, "field"), hit(2, "save"), hit(6, "title")],
            focus: Some(6),
        };
        let tree = snapshot.tree();
        assert_eq!(tree.nodes[0].1.children(), [node_id(6), node_id(2)]);
        assert_eq!(tree.nodes.len(), 3);
        assert_eq!(tree.nodes[1].1.label(), Some("field"));
        assert_eq!(tree.focus, node_id(6));
        snapshot.focus = Some(99);
        assert_eq!(snapshot.tree().focus, ROOT);
    }

    /// The same through AccessKit itself, once a UI Automation client has activated it.
    #[test]
    fn an_active_adapter_survives_duplicate_hits_and_a_stale_focus() {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPED,
        };
        use windows::core::w;
        // SAFETY: a hidden STATIC window owned by this test thread, destroyed at the end.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("a11y test"),
                WS_OVERLAPPED,
                0,
                0,
                100,
                100,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        let hit = |id| Hit {
            id,
            rect: Rect::new(0.0, 0.0, 50.0, 10.0),
            role: Role::Button,
            label: format!("hit {id}").into(),
            enabled: true,
            checked: None,
        };
        let mut a11y = A11y::deferred(hwnd, Role::Pane, "calendar".into());
        a11y.update(&[hit(6), hit(2)], Some(6));
        // A client asking for the window's UIA root (UiaRootObjectId) activates the adapter.
        let _: Option<LRESULT> = a11y.get_object(WPARAM(0), LPARAM(-25)).map(Into::into);
        a11y.update(&[hit(6), hit(2), hit(6)], Some(6));
        a11y.update(&[hit(2)], Some(6));
        // SAFETY: the window created above, on this thread.
        let _ = unsafe { DestroyWindow(hwnd) };
    }
}
