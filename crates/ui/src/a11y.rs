//! UI Automation through AccessKit. Every surface retains its latest hit snapshot; startup
//! surfaces can defer the adapter itself until WM_GETOBJECT. Neither constructor builds a tree
//! before a client asks, and updates do no UIA work while the adapter is inactive or deferred.

use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Role, Toggled, TreeId,
    TreeInfo, TreeUpdate,
};
use accesskit_windows::{Adapter, QueuedEvents};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::scene::{Hit, HitId};

/// Values and operations that a name alone cannot describe to assistive technology.
#[derive(Debug, Clone, PartialEq)]
pub enum Semantics {
    Range {
        value: f64,
        min: f64,
        max: f64,
        step: f64,
    },
    Text(Arc<str>),
    Reorder,
    Status(Arc<str>),
}

/// Rich actions remain owned data until the surface's UI thread consumes them.
#[derive(Debug, Clone, PartialEq)]
pub enum AccessibilityAction {
    SetNumber(f64),
    SetText(String),
    Increment,
    Decrement,
    Custom(i32),
}

type Pending = Arc<Mutex<VecDeque<(HitId, AccessibilityAction)>>>;

/// Posted to a surface when assistive technology acts on one of its hit items:
/// `wParam` is 0 for click (invoke) and 1 for focus, `lParam` the hit id.
pub(crate) const WM_A11Y_ACTION: u32 = WM_APP + 0x3a11;

const ROOT: NodeId = NodeId(0);
const REORDER_BASE: u64 = 1 << 40;
const STATUS_BASE: u64 = 1 << 48;

fn status_node(id: HitId) -> NodeId {
    NodeId(STATUS_BASE + u64::from(id))
}

fn reorder_node(hit: HitId, direction: u64) -> NodeId {
    NodeId(REORDER_BASE + u64::from(hit) * 4 + direction)
}

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
    pub semantics: Vec<(HitId, Semantics)>,
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
        let mut children: Vec<_> = hits.iter().map(|h| node_id(h.id)).collect();
        children.extend(self.semantics.iter().filter_map(|(id, semantic)| {
            matches!(semantic, Semantics::Status(_)).then_some(status_node(*id))
        }));
        root.set_children(children);
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
            if let Some((_, semantic)) = self.semantics.iter().find(|(id, _)| *id == hit.id) {
                match semantic {
                    Semantics::Range {
                        value,
                        min,
                        max,
                        step,
                    } => {
                        node.set_numeric_value(*value);
                        node.set_min_numeric_value(*min);
                        node.set_max_numeric_value(*max);
                        node.set_numeric_value_step(*step);
                        if hit.enabled {
                            for action in [Action::SetValue, Action::Increment, Action::Decrement] {
                                node.add_action(action);
                            }
                        }
                    }
                    Semantics::Text(value) => {
                        node.set_value(&**value);
                        if hit.enabled {
                            node.add_action(Action::SetValue);
                        }
                    }
                    Semantics::Reorder if hit.enabled => {
                        node.add_action(Action::CustomAction);
                        node.set_custom_actions(vec![
                            accesskit::CustomAction {
                                id: 1,
                                description: "Move up".into(),
                            },
                            accesskit::CustomAction {
                                id: 2,
                                description: "Move down".into(),
                            },
                        ]);
                        // UIA has no generic custom-action pattern. Named Invoke children give
                        // Windows clients the same operations without inventing keyboard input.
                        node.set_children(vec![reorder_node(hit.id, 1), reorder_node(hit.id, 2)]);
                    }
                    _ => {}
                }
            }
            (node_id(hit.id), node)
        }));
        for hit in &hits {
            if !hit.enabled
                || !self
                    .semantics
                    .iter()
                    .any(|(id, semantic)| *id == hit.id && *semantic == Semantics::Reorder)
            {
                continue;
            }
            for (direction, label) in [(1, "Move up"), (2, "Move down")] {
                let mut node = Node::new(Role::Button);
                node.set_label(label);
                node.add_action(Action::Click);
                nodes.push((reorder_node(hit.id, direction), node));
            }
        }
        nodes.extend(self.semantics.iter().filter_map(|(id, semantic)| {
            let Semantics::Status(message) = semantic else {
                return None;
            };
            let mut node = Node::new(Role::Status);
            node.set_label(&**message);
            node.set_live(accesskit::Live::Polite);
            Some((status_node(*id), node))
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
    pending: Pending,
}

impl A11y {
    pub(crate) fn new(hwnd: HWND, role: Role, name: Arc<str>) -> Self {
        let mut a11y = Self::deferred(hwnd, role, name);
        a11y.adapter = Some(Adapter::new(
            hwnd,
            false,
            Actions {
                hwnd: hwnd.0 as isize,
                pending: a11y.pending.clone(),
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
                semantics: Vec::new(),
            }),
            pending: Arc::default(),
        }
    }

    /// New hit items or focus; the tree follows only once a client has asked for it. Raise the
    /// returned events after letting go of any borrow.
    #[cfg(test)]
    pub(crate) fn update(&mut self, hits: &[Hit], focus: Option<HitId>) -> Option<QueuedEvents> {
        self.update_full(hits, focus, &[])
    }

    pub(crate) fn update_full(
        &mut self,
        hits: &[Hit],
        focus: Option<HitId>,
        semantics: &[(HitId, Semantics)],
    ) -> Option<QueuedEvents> {
        if self.snapshot.hits == hits
            && self.snapshot.focus == focus
            && self.snapshot.semantics == semantics
        {
            return None;
        }
        let next = Rc::new(Snapshot {
            hits: hits.to_vec(),
            focus,
            semantics: semantics.to_vec(),
            ..(*self.snapshot).clone()
        });
        self.snapshot = next.clone();
        self.adapter.as_mut()?.update_if_active(|| next.tree())
    }

    pub(crate) fn take_action(&self, hit: HitId) -> Option<AccessibilityAction> {
        let mut pending = self.pending.lock().ok()?;
        let at = pending.iter().position(|(id, _)| *id == hit)?;
        pending.remove(at).map(|(_, action)| action)
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
        let pending = self.pending.clone();
        self.adapter
            .get_or_insert_with(|| {
                Adapter::new(
                    hwnd,
                    false,
                    Actions {
                        hwnd: hwnd.0 as isize,
                        pending,
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
    pending: Pending,
}

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        if let Some(encoded) = request.target_node.0.checked_sub(REORDER_BASE) {
            if request.action != Action::Click || !matches!(encoded % 4, 1 | 2) {
                return;
            }
            let Ok(hit) = u32::try_from(encoded / 4) else {
                return;
            };
            if let Ok(mut pending) = self.pending.lock() {
                if pending.len() >= 128 {
                    pending.pop_front();
                }
                pending.push_back((hit, AccessibilityAction::Custom((encoded % 4) as i32)));
            } else {
                return;
            }
            // SAFETY: posts only to the surface that owns this adapter.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(self.hwnd as *mut _)),
                    WM_A11Y_ACTION,
                    WPARAM(0),
                    LPARAM(hit as isize),
                )
            };
            return;
        }
        let Some(hit) = request
            .target_node
            .0
            .checked_sub(1)
            .and_then(|id| u32::try_from(id).ok())
        else {
            return;
        };
        let rich = match (request.action, request.data) {
            (Action::SetValue, Some(accesskit::ActionData::NumericValue(value))) => {
                Some(AccessibilityAction::SetNumber(value))
            }
            (Action::SetValue, Some(accesskit::ActionData::Value(value))) => {
                Some(AccessibilityAction::SetText(value.into()))
            }
            (Action::Increment, _) => Some(AccessibilityAction::Increment),
            (Action::Decrement, _) => Some(AccessibilityAction::Decrement),
            (Action::CustomAction, Some(accesskit::ActionData::CustomAction(id))) => {
                Some(AccessibilityAction::Custom(id))
            }
            _ => None,
        };
        if let Some(action) = rich {
            if let Ok(mut pending) = self.pending.lock() {
                // Bound abandoned requests when a client targets a closing surface.
                if pending.len() >= 128 {
                    pending.pop_front();
                }
                pending.push_back((hit, action));
            } else {
                return;
            }
        }
        let kind = match request.action {
            Action::Click => 0,
            Action::Focus => 1,
            Action::SetValue | Action::Increment | Action::Decrement | Action::CustomAction => 0,
            _ => return,
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
            semantics: vec![],
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
            semantics: vec![],
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

#[cfg(test)]
mod semantic_tests {
    use super::*;
    #[test]
    fn ranges_fields_reordering_and_live_feedback_expose_semantic_contracts() {
        let hit = |id, role| Hit {
            id,
            rect: crate::Rect::new(0.0, 0.0, 100.0, 30.0),
            role,
            label: "control".into(),
            enabled: true,
            checked: None,
        };
        let snapshot = Snapshot {
            role: Role::Window,
            name: "Studio".into(),
            hits: vec![
                hit(1, Role::Slider),
                hit(2, Role::TextInput),
                hit(3, Role::ListItem),
            ],
            focus: None,
            semantics: vec![
                (
                    1,
                    Semantics::Range {
                        value: 0.5,
                        min: 0.0,
                        max: 1.0,
                        step: 0.01,
                    },
                ),
                (2, Semantics::Text("hello".into())),
                (3, Semantics::Reorder),
                (u32::MAX - 1, Semantics::Status("Saved".into())),
            ],
        };
        let tree = snapshot.tree();
        let node = |id| {
            &tree
                .nodes
                .iter()
                .find(|(node, _)| *node == node_id(id))
                .unwrap()
                .1
        };
        assert_eq!(node(1).numeric_value(), Some(0.5));
        assert_eq!(node(1).min_numeric_value(), Some(0.0));
        assert_eq!(node(1).max_numeric_value(), Some(1.0));
        assert!(node(1).supports_action(Action::SetValue));
        assert_eq!(node(2).value(), Some("hello"));
        assert!(node(2).supports_action(Action::SetValue));
        assert_eq!(node(3).custom_actions().len(), 2);
        let status = &tree
            .nodes
            .iter()
            .find(|(id, _)| *id == status_node(u32::MAX - 1))
            .unwrap()
            .1;
        assert_eq!(status.live(), Some(accesskit::Live::Polite));
    }

    #[test]
    fn status_nodes_cannot_collide_with_disabled_controls() {
        let id = u32::MAX - 2;
        let tree = Snapshot {
            role: Role::Window,
            name: "Studio".into(),
            hits: vec![Hit {
                id,
                rect: crate::Rect::default(),
                role: Role::Button,
                label: "Undo".into(),
                enabled: false,
                checked: None,
            }],
            focus: None,
            semantics: vec![(id, Semantics::Status("Saved".into()))],
        }
        .tree();
        let children = tree.nodes[0].1.children();
        assert_eq!(children.len(), 2);
        assert_ne!(children[0], children[1]);
        let unique: std::collections::HashSet<_> = tree.nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(unique.len(), tree.nodes.len());
    }
}
