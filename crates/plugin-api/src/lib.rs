//! The plugin platform's contract (docs/plugins.md): the WIT world in `wit/`, the binary frames
//! the shell and the `mypaper-plugins` helper exchange, and view diffing.

pub use mypaper_plugin_sdk::{Face, SizeClass, View};

/// The WIT package this host implements (`wit/plugin.wit`).
pub const WIT_PACKAGE: &str = "mypaper:plugin@0.1.0";
/// The `api` major.minor a manifest must name.
pub const API_MAJOR: u32 = 0;
pub const API_MINOR: u32 = 1;

pub mod wire {
    //! `[u32 LE length][postcard message]` frames on the helper's stdin/stdout.
    use std::io::{self, Read, Write};

    use serde::{Deserialize, Serialize, de::DeserializeOwned};

    use super::SizeClass;

    /// Frames larger than this are refused (a face is a few KiB; images use shared memory).
    pub const MAX_FRAME: usize = 4 << 20;

    /// Shell → helper.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub enum HostMsg {
        /// A face (or bar module / panel) was placed or re-sized; `dir` is the plugin's folder and
        /// `settings` the instance's settings as JSON.
        Open { plugin: String, dir: String, instance: String, face: String, size: SizeClass, settings: String },
        Close { instance: String },
        Action { instance: String, action: String, value: Option<String> },
        /// A `commands` or `menu` contribution was invoked (a shortcut, a desktop-menu item). `dir`
        /// is the plugin's folder, so the helper loads a plugin no face has opened yet.
        Command { plugin: String, dir: String, id: String },
        /// The desktop is held (fullscreen, locked) or released.
        Held(bool),
        /// Asks for [`HelperMsg::Stats`] of every loaded plugin.
        Stats,
        Shutdown,
        /// A system event; the helper delivers it only to plugins that subscribed to its topic and
        /// hold its permission (`windows` for window events), and coalesces moves to one a frame.
        System(SystemEvent),
        /// Drop a plugin and its outstanding work before uninstalling its files.
        Remove { plugin: String, acknowledge: bool },
    }

    /// How a plugin is doing, shown in its faces' footer.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub enum Status {
        Live,
        /// It trapped; its faces keep their last view until it's back.
        Restarting(String),
        /// Too many traps, or it failed to load; faces show the reason.
        Stopped(String),
        /// Saved and dropped because nothing of it is visible.
        Unloaded,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub enum TokenValue {
        Color(String),
        Number(f32),
        /// A theme role name (`accent`, …).
        Tone(String),
        Flag(bool),
    }

    /// `style.apply` from a plugin, for `surface` (`top-bar`, `dock`, …).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct StylePatch {
        pub surface: String,
        pub tokens: Vec<(String, TokenValue)>,
    }

    /// What the shell observed, for plugins subscribed to the topic (docs/plugins.md, "Events").
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub enum SystemEvent {
        /// `id` is the window handle; `process` the image name (`code.exe`).
        Window { change: WindowChange, id: u64, title: String, process: String },
        Monitors,
        /// The notification as JSON (app, title, body), from the shell's listener.
        Notification(String),
        /// The media session as JSON (title, artist, playing), from the shell's media provider.
        Media(String),
        Power,
        Theme,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum WindowChange {
        Opened,
        Closed,
        Focused,
        Moved,
    }

    /// Helper → shell.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub enum HelperMsg {
        /// `face` is the JSON of [`super::Face`] (the kit tree the shell renderer reads); sent only
        /// when it changed. `changed` counts the replaced subtrees ([`super::diff`]).
        View { instance: String, face: Vec<u8>, changed: u32 },
        Status { plugin: String, status: Status },
        /// `None` resets the surface.
        Style { plugin: String, surface: String, patch: Option<StylePatch> },
        Stats { plugin: String, memory: u64, cpu_us: u64, events: u64 },
        Removed { plugin: String },
        /// Visible notification subscribers, already checked against manifest permissions.
        Notifications { plugins: Vec<String> },
        /// The system-event topics (`windows`, `monitors`, `media`, `power`, `theme`) at least one
        /// visible, permitted plugin subscribes to: the shell forwards only these (the lease).
        Demand { topics: Vec<String> },
    }

    pub fn write<T: Serialize>(out: &mut impl Write, message: &T) -> io::Result<()> {
        let bytes = postcard::to_stdvec(message).map_err(io::Error::other)?;
        out.write_all(&(bytes.len() as u32).to_le_bytes())?;
        out.write_all(&bytes)?;
        out.flush()
    }

    /// The next frame, or `None` at a clean end of stream.
    pub fn read<T: DeserializeOwned>(input: &mut impl Read) -> io::Result<Option<T>> {
        let mut len = [0; 4];
        match input.read_exact(&mut len) {
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            other => other?,
        }
        let len = u32::from_le_bytes(len) as usize;
        if len > MAX_FRAME {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("frame of {len} bytes")));
        }
        let mut bytes = vec![0; len];
        input.read_exact(&mut bytes)?;
        postcard::from_bytes(&bytes).map(Some).map_err(io::Error::other)
    }
}

/// The direct children of a node and the node without them, for comparing payloads.
fn split(view: &View) -> (View, &[View]) {
    use View::*;
    match view {
        List { children, gap } => (List { children: Vec::new(), gap: *gap }, children),
        Stack { children, gap } => (Stack { children: Vec::new(), gap: *gap }, children),
        Row { children, gap } => (Row { children: Vec::new(), gap: *gap }, children),
        Grid { children, columns, gap } => (Grid { children: Vec::new(), columns: *columns, gap: *gap }, children),
        Card { title, child } => (Card { title: title.clone(), child: Box::new(Spacer) }, std::slice::from_ref(&**child)),
        leaf => (leaf.clone(), &[]),
    }
}

/// The subtrees of `new` that differ from `old`, as child-index paths from the root (`[]` is the
/// whole tree). A node whose kind, payload or child count changed is replaced whole; otherwise its
/// children are compared one by one. Empty means nothing to repaint.
pub fn diff(old: &View, new: &View) -> Vec<Vec<u16>> {
    fn walk(old: &View, new: &View, path: &mut Vec<u16>, out: &mut Vec<Vec<u16>>) {
        let ((old_node, old_children), (new_node, new_children)) = (split(old), split(new));
        if old_node != new_node || old_children.len() != new_children.len() {
            out.push(path.clone());
            return;
        }
        for (i, (a, b)) in old_children.iter().zip(new_children).enumerate() {
            path.push(i as u16);
            walk(a, b, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(old, new, &mut Vec::new(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mypaper_plugin_sdk::Role;

    fn face(label: &str) -> View {
        View::stack([View::text("Title", Role::Headline), View::row([View::text(label, Role::Body), View::button("Go", "go")])])
    }

    #[test]
    fn identical_trees_have_no_changes() {
        assert!(diff(&face("a"), &face("a")).is_empty());
    }

    #[test]
    fn a_changed_leaf_is_the_only_change() {
        assert_eq!(diff(&face("a"), &face("b")), vec![vec![1, 0]]);
    }

    #[test]
    fn a_changed_child_count_replaces_the_parent() {
        let more = View::stack([View::text("Title", Role::Headline)]);
        assert_eq!(diff(&face("a"), &more), vec![Vec::<u16>::new()]);
        let card = |t: &str| View::Card { title: None, child: Box::new(View::text(t, Role::Body)) };
        assert_eq!(diff(&card("x"), &card("y")), vec![vec![0]]);
    }

    #[test]
    fn frames_round_trip() {
        use wire::*;
        let mut bytes = Vec::new();
        let open = HostMsg::Open { plugin: "hello".into(), dir: "d".into(), instance: "i".into(), face: "main".into(), size: SizeClass::Large, settings: "{}".into() };
        write(&mut bytes, &open).unwrap();
        write(&mut bytes, &HostMsg::Held(true)).unwrap();
        let mut input = bytes.as_slice();
        assert_eq!(read::<HostMsg>(&mut input).unwrap(), Some(open));
        assert_eq!(read::<HostMsg>(&mut input).unwrap(), Some(HostMsg::Held(true)));
        assert_eq!(read::<HostMsg>(&mut input).unwrap(), None);
    }
}
