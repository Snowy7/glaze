//! `widgets.json` (where every widget instance sits) and the grid it sits on
//! (docs/widgets.md, "Layout").
//!
//! Pure: no Win32. Lengths are DIPs (CSS pixels) relative to the monitor's
//! top-left corner unless a name says otherwise.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use mypaper_theme::Appearance;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::manifest::{WidgetManifest, valid_id};
use crate::{look, pick};

/// Version 2 halved the grid cell: version-1 files are upgraded by [`Layout::upgrade`].
pub const VERSION: u32 = 2;

/// Grid cells per desktop-icon cell (layout version 2; version 1 used the icon cell itself).
/// The fine grid subdivides the icon grid exactly, so `cell × 2, size × 2` lands a version-1
/// widget on the very same rectangle.
pub const SUBDIVISION: u32 = 2;

/// The cell size when the icon spacing is unknown.
const FALLBACK_CELL: f64 = 96.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub grid: GridSpec,
    #[serde(default)]
    pub instances: Vec<Instance>,
    /// Whether the overlay layer is shown (docs/widgets.md, "Placement presets"); the tray,
    /// the hotkey and `--overlay` flip it.
    #[serde(default = "overlay_on")]
    pub overlay: bool,
    /// The user's look for every widget (the "user" layer of docs/widgets.md, "Appearance").
    #[serde(default, skip_serializing_if = "is_blank", serialize_with = "layer")]
    pub appearance: Appearance,
}

fn version() -> u32 {
    VERSION
}

fn overlay_on() -> bool {
    true
}

/// An appearance layer that sets nothing isn't written.
fn is_blank(appearance: &Appearance) -> bool {
    *appearance == Appearance::default()
}

/// Writes only the fields a layer sets (the derived impl would write `null` for the rest).
fn layer<S: Serializer>(appearance: &Appearance, serializer: S) -> Result<S::Ok, S::Error> {
    look::json_of(appearance).serialize(serializer)
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            version: VERSION,
            grid: GridSpec::default(),
            instances: Vec::new(),
            overlay: true,
            appearance: Appearance::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GridSpec {
    /// Cell size in DIPs; 0 follows the desktop icon spacing.
    pub cell: f64,
    pub gap: f64,
    pub margin: f64,
}

impl Default for GridSpec {
    fn default() -> Self {
        Self {
            cell: 0.0,
            gap: 8.0,
            margin: 16.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub instance: String,
    pub widget: String,
    /// The monitor's device name, e.g. `\\.\DISPLAY1`.
    pub monitor: String,
    pub cell: [u32; 2],
    pub size: [u32; 2],
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub settings: Map<String, Value>,
    /// This instance's own look (the top layer of docs/widgets.md, "Appearance").
    #[serde(default, skip_serializing_if = "is_blank", serialize_with = "layer")]
    pub appearance: Appearance,
}

impl Layout {
    /// Loads the layout; a missing file is an empty layout.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).with_context(|| format!("in {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn parse(json: &str) -> Result<Self> {
        // Said first, and plainly: a newer file is not a broken one (it must never be "started
        // over"). Readers tell the two apart with [`newer_version`].
        if let Some(version) = newer_version(json) {
            bail!("{NEWER} (layout version {version}; this mypaper reads up to {VERSION})");
        }
        let mut layout: Self = serde_json::from_str(json)?;
        layout.instances.retain(|instance| {
            let valid = valid_id(&instance.instance) && valid_id(&instance.widget);
            if !valid {
                log::error!("widgets.json: skipping instance {:?} (widget {:?}): instance and widget ids must match [a-z0-9-], start with a letter or digit, and be at most 64 characters; other widgets remain editable", instance.instance, instance.widget);
            }
            valid
        });
        layout.validate()?;
        Ok(layout)
    }

    /// Loads `path` and moves a version-1 file onto the fine grid ([`Layout::upgrade`]). Before
    /// an upgraded layout can ever be written, the old file is kept byte for byte as
    /// `widgets.json.v1` (never overwritten; a different one goes to `.v1.1`, ...). Returns the
    /// layout and whether it was upgraded (and so should be saved).
    pub fn load_upgrading(path: &Path) -> Result<(Self, bool)> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Self::default(), false));
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let text = std::str::from_utf8(&bytes).with_context(|| format!("in {}", path.display()))?;
        let mut layout = Self::parse(text).with_context(|| format!("in {}", path.display()))?;
        if layout.version < VERSION {
            crate::files::backup_tagged(path, &format!("v{}", layout.version), &bytes)
                .context("keeping the layout before its upgrade")?;
        }
        let upgraded = layout.upgrade();
        Ok((layout, upgraded))
    }

    /// Moves a version-1 layout onto the fine grid: every cell and size doubles, which keeps
    /// each widget exactly where and as big as it was (see [`SUBDIVISION`]). The shell calls
    /// this after every load and saves when it returns true; Studio edits the raw file in
    /// whatever version it finds, so parsing never upgrades on its own.
    pub fn upgrade(&mut self) -> bool {
        if self.version >= VERSION {
            return false;
        }
        for instance in &mut self.instances {
            instance.cell = instance.cell.map(|c| c.saturating_mul(SUBDIVISION));
            instance.size = instance.size.map(|s| s.saturating_mul(SUBDIVISION));
        }
        self.version = VERSION;
        true
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            (1..=VERSION).contains(&self.version),
            "version {} isn't supported (expected 1 to {VERSION})",
            self.version
        );
        let GridSpec { cell, gap, margin } = self.grid;
        ensure!(
            cell == 0.0 || (16.0..=1024.0).contains(&cell),
            "grid.cell must be 0 (icon grid) or 16..1024"
        );
        ensure!((0.0..=256.0).contains(&gap), "grid.gap must be 0..256");
        ensure!(
            (0.0..=1024.0).contains(&margin),
            "grid.margin must be 0..1024"
        );
        self.appearance
            .validate()
            .context("in the top-level appearance")?;
        let mut ids = HashSet::new();
        for instance in &self.instances {
            let id = &instance.instance;
            ensure!(
                valid_id(id),
                "instance id {id:?} must be lowercase [a-z0-9-]"
            );
            ensure!(ids.insert(id), "instance id {id:?} appears twice");
            ensure!(
                valid_id(&instance.widget),
                "{id}: widget id {:?} is invalid",
                instance.widget
            );
            ensure!(
                instance.size.iter().all(|&s| s >= 1),
                "{id}: size must be at least [1, 1]"
            );
            instance
                .appearance
                .validate()
                .with_context(|| format!("in {id}"))?;
        }
        Ok(())
    }

    /// Writes atomically, so watchers never see a half-written file.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        Self::save_value(&serde_json::to_value(self)?, path)
    }

    /// Studio keeps omitted defaults and raw settings while sharing the desktop's write guard.
    pub fn save_value(value: &Value, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(value)? + "\n";
        let next = Self::parse(&text)?;
        // An external editor may have changed the file since the host loaded it. Check at the
        // write boundary too, so even an edit arriving before the watcher cannot destroy it.
        match std::fs::read(path) {
            Ok(bytes) => {
                // A newer mypaper's file, or an older layout version over a newer one: refused
                // as is (no backup: nothing is wrong with the file).
                let current = std::str::from_utf8(&bytes).ok();
                if let Some(version) = current.and_then(newer_version) {
                    bail!("{NEWER} (layout version {version}); restart to update before editing");
                }
                let on_disk = current.and_then(file_version).unwrap_or(0);
                ensure!(
                    on_disk <= u64::from(next.version),
                    "refusing to write layout version {} over version {on_disk}",
                    next.version
                );
                if let Err(error) = std::str::from_utf8(&bytes)
                    .map_err(anyhow::Error::from)
                    .and_then(Self::parse)
                {
                    let backup = crate::files::backup(path, &bytes)?;
                    bail!(
                        "refusing to overwrite invalid {}: {error:#}; backup: {}",
                        path.display(),
                        backup.display()
                    );
                }
                crate::files::write_atomic(&path.with_extension("json.previous"), &bytes)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("checking existing layout before saving"),
        }
        crate::files::write_atomic(path, text.as_bytes())
    }

    pub fn get(&self, instance: &str) -> Option<&Instance> {
        self.instances.iter().find(|i| i.instance == instance)
    }

    fn get_mut(&mut self, instance: &str) -> Result<&mut Instance> {
        self.instances
            .iter_mut()
            .find(|i| i.instance == instance)
            .with_context(|| format!("no instance {instance:?}"))
    }

    /// Instances of each widget, over all monitors.
    pub fn counts(&self) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::new();
        for instance in &self.instances {
            *counts.entry(instance.widget.as_str()).or_default() += 1;
        }
        counts
    }

    /// How many instances of `widget` exist, and how many of those `here` accepts (the ones on
    /// one monitor): what the manifest's `instances` rule needs to decide an add.
    pub fn count_of(&self, widget: &str, here: impl Fn(&Instance) -> bool) -> (usize, usize) {
        let of_widget = self.instances.iter().filter(|i| i.widget == widget);
        (
            of_widget.clone().count(),
            of_widget.filter(|i| here(i)).count(),
        )
    }

    /// `<widget>-<n>` with the smallest unused `n`.
    pub fn new_id(&self, widget: &str) -> String {
        (1..)
            .map(|n| format!("{widget}-{n}"))
            .find(|id| self.get(id).is_none())
            .expect("an unused id exists")
    }

    /// Moves and resizes existing instances.
    pub fn apply(&mut self, changes: &[(String, [u32; 2], [u32; 2])]) -> Result<()> {
        for (instance, cell, size) in changes {
            let instance = self.get_mut(instance)?;
            instance.cell = *cell;
            instance.size = *size;
        }
        Ok(())
    }

    /// Adds an instance of `widget` and returns its id.
    pub fn add(&mut self, widget: &str, monitor: &str, cell: [u32; 2], size: [u32; 2]) -> String {
        let id = self.new_id(widget);
        self.instances.push(Instance {
            instance: id.clone(),
            widget: widget.to_owned(),
            monitor: monitor.to_owned(),
            cell,
            size,
            settings: Map::new(),
            appearance: Appearance::default(),
        });
        id
    }

    /// The first-run layout: a column of widgets down the right edge of
    /// `monitor` (desktop icons fill from the left), stacked top to bottom
    /// while they fit in `cols` x `rows`. `widgets` is `(id, default size)`.
    pub fn starter(monitor: &str, cols: u32, rows: u32, widgets: &[(&str, [u32; 2])]) -> Self {
        let mut layout = Self::default();
        let mut row = 0;
        for &(widget, [w, h]) in widgets {
            if w > cols || row + h > rows {
                continue;
            }
            layout.add(widget, monitor, [cols - w, row], [w, h]);
            row += h;
        }
        layout
    }

    /// Copies an instance (settings included, storage not) to `cell`.
    pub fn duplicate(&mut self, instance: &str, cell: [u32; 2]) -> Result<String> {
        let mut copy = self
            .get(instance)
            .with_context(|| format!("no instance {instance:?}"))?
            .clone();
        copy.instance = self.new_id(&copy.widget);
        for value in copy.settings.values_mut() {
            pick::rebase_value(value, instance, &copy.instance);
        }
        copy.cell = cell;
        let id = copy.instance.clone();
        self.instances.push(copy);
        Ok(id)
    }

    pub fn remove(&mut self, instance: &str) -> Result<()> {
        let before = self.instances.len();
        self.instances.retain(|i| i.instance != instance);
        if self.instances.len() == before {
            bail!("no instance {instance:?}");
        }
        Ok(())
    }

    /// Moves instances of a widget that was merged into another onto that one (docs/widgets.md,
    /// "Replaced widgets"). `replacement(old)` is the new widget and the settings the old one
    /// implies; they win over the instance's saved values. Picked files keep working: their
    /// URLs move to the new widget's origin (the files live in the instance's folder). The
    /// instance id stays, and with it the instance's storage. Returns whether anything moved.
    pub fn migrate<'a>(
        &mut self,
        replacement: impl Fn(&str) -> Option<(&'a str, &'a Map<String, Value>)>,
    ) -> bool {
        let mut moved = false;
        for instance in &mut self.instances {
            let Some((widget, implied)) = replacement(&instance.widget) else {
                continue;
            };
            log::info!(
                "{}: {} was merged into {widget}",
                instance.instance,
                instance.widget
            );
            for value in instance.settings.values_mut() {
                let rebased = value
                    .as_str()
                    .and_then(pick::parse_data_url)
                    .filter(|url| url.widget == instance.widget)
                    .map(|url| pick::data_url(widget, &url.instance, &url.name));
                if let Some(url) = rebased {
                    *value = Value::String(url);
                }
            }
            instance.settings.extend(implied.clone());
            instance.widget = widget.to_owned();
            moved = true;
        }
        moved
    }

    /// Merges setting values into an instance's saved settings.
    pub fn set_settings(&mut self, instance: &str, values: Map<String, Value>) -> Result<()> {
        self.get_mut(instance)?.settings.extend(values);
        Ok(())
    }

    /// Edits an appearance layer: `instance`'s own, or the user's for every widget when `None`.
    /// Fields with a value are set; `null` fields are cleared (see [`look::patch`]).
    pub fn set_appearance(
        &mut self,
        instance: Option<&str>,
        values: &Map<String, Value>,
    ) -> Result<()> {
        let target = match instance {
            Some(instance) => &mut self.get_mut(instance)?.appearance,
            None => &mut self.appearance,
        };
        *target = look::patch(target, values)?;
        Ok(())
    }
}

/// What a reader says of a layout from a newer mypaper.
pub const NEWER: &str = "This layout was saved by a newer mypaper";

/// The `version` a layout file declares, if it declares one.
fn file_version(json: &str) -> Option<u64> {
    serde_json::from_str::<Value>(json).ok()?.get("version")?.as_u64()
}

/// The version of a layout saved by a newer mypaper than this one (above [`VERSION`]). Such a
/// file is fine: a reader shows "restart to update" and never offers to start over.
pub fn newer_version(json: &str) -> Option<u64> {
    file_version(json).filter(|&v| v > u64::from(VERSION))
}

/// A rectangle in DIPs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl RectF {
    /// The physical-pixel rect (left, top, right, bottom) that surely covers
    /// this one at `scale`: rounded outwards, plus a pixel of slack, so the
    /// click-through region never clips a widget's edge.
    pub fn to_pixels(self, scale: f64) -> [i32; 4] {
        [
            (self.x * scale).floor() as i32 - 1,
            (self.y * scale).floor() as i32 - 1,
            ((self.x + self.width) * scale).ceil() as i32 + 1,
            ((self.y + self.height) * scale).ceil() as i32 + 1,
        ]
    }
}

/// The icon spacing from `WindowMetrics` (`IconSpacing` or
/// `IconVerticalSpacing`) as DIPs. Negative values are twips
/// (`px = -value / 1440 × DPI`, so `-value / 15` at 96 DPI), positive ones
/// pixels at 96 DPI; anything missing or absurd falls back to 96.
pub fn icon_spacing_dip(raw: Option<i32>) -> f64 {
    let dip = match raw {
        Some(value) if value < 0 => f64::from(value) / -15.0,
        Some(value) if value > 0 => f64::from(value),
        _ => FALLBACK_CELL,
    };
    if (32.0..=512.0).contains(&dip) {
        dip
    } else {
        FALLBACK_CELL
    }
}

/// One monitor's grid.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Grid {
    /// The top-left of cell `[0, 0]`.
    pub origin: [f64; 2],
    /// `[width, height]` of a cell.
    pub cell: [f64; 2],
    pub gap: f64,
    pub cols: u32,
    pub rows: u32,
}

impl Grid {
    /// The grid for a work area: as many whole icon cells (`spec.cell`, or `icon_cell` when
    /// it's 0) as fit inside the margin, stretched so they span the area edge to edge, then
    /// each split into [`SUBDIVISION`] cells per side. Whole cells always fill the area exactly:
    /// no leftover strip on the right or bottom.
    pub fn new(spec: &GridSpec, icon_cell: [f64; 2], work: RectF) -> Self {
        let cell = if spec.cell > 0.0 {
            [spec.cell; 2]
        } else {
            icon_cell
        };
        let span = |length: f64, nominal: f64| {
            let margin = spec.margin.min((length - 1.0).max(0.0) / 2.0);
            let usable = (length - 2.0 * margin).max(1.0);
            let coarse = (((usable + spec.gap) / (nominal + spec.gap)).floor() as u32).max(1);
            let size = (usable - f64::from(coarse - 1) * spec.gap) / f64::from(coarse);
            // A cell too small to split (a tiny area) stays whole.
            let split = if size - spec.gap >= 2.0 { SUBDIVISION } else { 1 };
            let count = coarse * split;
            let size = (usable - f64::from(count - 1) * spec.gap) / f64::from(count);
            (count, size, margin)
        };
        let (cols, cell_w, margin_x) = span(work.width, cell[0]);
        let (rows, cell_h, margin_y) = span(work.height, cell[1]);
        Self {
            origin: [work.x + margin_x, work.y + margin_y],
            cell: [cell_w, cell_h],
            gap: spec.gap,
            cols,
            rows,
        }
    }

    /// Where a widget at `cell`, `size` cells big, is drawn.
    pub fn rect(&self, cell: [u32; 2], size: [u32; 2]) -> RectF {
        let at = |i: usize| self.origin[i] + f64::from(cell[i]) * (self.cell[i] + self.gap);
        let span = |i: usize| {
            f64::from(size[i]) * self.cell[i] + f64::from(size[i].saturating_sub(1)) * self.gap
        };
        RectF {
            x: at(0),
            y: at(1),
            width: span(0),
            height: span(1),
        }
    }

    /// Keeps a widget on the grid: the size shrinks to the grid if needed,
    /// then the cell moves in so the whole widget fits.
    pub fn clamp(&self, cell: [u32; 2], size: [u32; 2]) -> ([u32; 2], [u32; 2]) {
        let limits = [self.cols, self.rows];
        let size = [0, 1].map(|i| size[i].clamp(1, limits[i]));
        let cell = [0, 1].map(|i| cell[i].min(limits[i] - size[i]));
        (cell, size)
    }
}

/// An instance as shown: clamped, with resolved settings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Placed {
    pub instance: String,
    pub widget: String,
    pub cell: [u32; 2],
    pub size: [u32; 2],
    pub settings: Map<String, Value>,
    /// What the widget shows: every layer merged, every field set (see [`look::resolve`]).
    pub appearance: Map<String, Value>,
    /// Only the fields the instance sets itself (for the edit-mode panel).
    pub own: Map<String, Value>,
}

/// Places an instance on `grid`: the size clamped to the manifest and the
/// grid, the cell to the grid, its settings resolved, and its look taken from
/// `resolved` (already layered by [`look::resolve`]).
pub fn place(
    instance: &Instance,
    manifest: &WidgetManifest,
    grid: &Grid,
    resolved: &Appearance,
) -> Placed {
    let size = manifest
        .fit_size(instance.size.map(f64::from), [grid.cols, grid.rows])
        .unwrap_or(manifest.size.default);
    let (cell, size) = grid.clamp(instance.cell, size);
    Placed {
        instance: instance.instance.clone(),
        widget: instance.widget.clone(),
        cell,
        size,
        settings: manifest.resolve_settings(&instance.settings),
        appearance: look::json_of(resolved),
        own: look::json_of(&instance.appearance),
    }
}

/// Which of `devices` an instance saved on `monitor` shows on: that monitor,
/// or the primary one (`devices[primary]`) when it's missing.
pub fn show_on<'a>(monitor: &str, devices: &'a [String], primary: usize) -> Option<&'a str> {
    devices
        .iter()
        .find(|d| d.as_str() == monitor)
        .or_else(|| devices.get(primary))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_ids_skip_only_the_bad_instance_and_edits_still_work() {
        let mut layout = Layout::parse(r#"{"instances":[
            {"instance":"bad-1","widget":"weather.now","monitor":"x","cell":[0,0],"size":[1,1]},
            {"instance":"also.bad","widget":"clock","monitor":"x","cell":[0,0],"size":[1,1]},
            {"instance":"clock-1","widget":"clock","monitor":"x","cell":[0,0],"size":[1,1]}
        ]}"#).unwrap();
        assert_eq!(layout.instances.len(), 1);
        assert_eq!(layout.instances[0].instance, "clock-1");
        layout.apply(&[("clock-1".into(), [2, 3], [2, 1])]).unwrap();
        layout.validate().unwrap();
        assert_eq!(layout.instances[0].cell, [2, 3]);
        let root = crate::files::temp_dir("invalid-layout-instance");
        let path = root.join("widgets.json");
        layout.save(&path).unwrap();
        assert_eq!(Layout::load(&path).unwrap().instances.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn saving_after_external_corruption_preserves_the_exact_file_and_backup() {
        let dir = crate::files::temp_dir("invalid-layout-save");
        let path = dir.join("widgets.json");
        let mut layout = Layout::default();
        layout.save(&path).unwrap();
        for bytes in [b"{ broken".as_slice(), b"{\"version\":0}", b"\xff\xfe"] {
            std::fs::write(&path, bytes).unwrap();
            layout.overlay = !layout.overlay;
            assert!(layout.save(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert!(
                std::fs::read_dir(&dir)
                    .unwrap()
                    .flatten()
                    .any(|entry| entry.path() != path
                        && std::fs::read(entry.path()).is_ok_and(|saved| saved == bytes))
            );
        }
        // Correcting the file releases the write gate, without losing recovery copies.
        std::fs::write(&path, b"{\"instances\":[]}").unwrap();
        layout.save(&path).unwrap();
        assert_eq!(Layout::load(&path).unwrap(), layout);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_rebases_owned_pictures_and_survives_removing_the_source() {
        let dir = crate::files::temp_dir("duplicate-photo");
        let source = dir.join("a photo.png");
        std::fs::write(&source, b"picture bytes").unwrap();
        let mut layout = Layout::default();
        let id = layout.add("photo", "m", [0, 0], [2, 2]);
        let picked = pick::import(&dir, "photo", &id, &source).unwrap();
        layout
            .set_settings(
                &id,
                serde_json::json!({"image1": picked.url,
            "nested": [{"url": picked.url}], "external": "https://example.com/photo.jpg"})
                .as_object()
                .unwrap()
                .clone(),
            )
            .unwrap();
        let copy = layout.duplicate(&id, [2, 0]).unwrap();
        pick::copy_instance(&dir, &id, &copy);
        layout.remove(&id).unwrap();
        pick::remove_instance(&dir, &id);
        let settings = &layout.get(&copy).unwrap().settings;
        for value in [&settings["image1"], &settings["nested"][0]["url"]] {
            let url = pick::parse_data_url(value.as_str().unwrap()).unwrap();
            assert_eq!(url.instance, copy);
            assert_eq!(
                std::fs::read(dir.join(&url.instance).join("files").join(&url.name)).unwrap(),
                b"picture bytes"
            );
        }
        assert_eq!(settings["external"], "https://example.com/photo.jpg");
        std::fs::remove_dir_all(dir).unwrap();
    }

    const LAYOUT: &str = r#"{
        "version": 1,
        "grid": { "cell": 0, "gap": 8, "margin": 16 },
        "instances": [
            { "instance": "clock-1", "widget": "clock", "monitor": "\\\\.\\DISPLAY2",
              "cell": [0, 0], "size": [2, 1], "settings": { "format24": true } }
        ]
    }"#;

    fn manifest() -> WidgetManifest {
        WidgetManifest::parse(
            r#"{ "id": "clock", "name": "Clock", "size": { "default": [2, 1], "max": [4, 2] },
                 "settings": [{ "key": "format24", "type": "boolean", "label": "24h", "default": false }] }"#,
            "clock",
        )
        .unwrap()
    }

    #[test]
    fn migrate_moves_replaced_instances_and_rebases_their_files() {
        let mut layout = Layout::parse(
            r#"{ "instances": [
                { "instance": "photo-frame-1", "widget": "photo-frame", "monitor": "m", "cell": [0, 0], "size": [3, 2],
                  "settings": { "variant": "matte", "shuffle": true,
                                "image1": "https://photo-frame.widget.mypaper/__data/photo-frame-1/a.jpg" } },
                { "instance": "clock-1", "widget": "clock", "monitor": "m", "cell": [3, 0], "size": [2, 1] }
            ] }"#,
        )
        .unwrap();
        let implied = Map::from_iter([("variant".to_owned(), Value::from("polaroid"))]);
        let replacement = |old: &str| (old == "photo-frame").then_some(("photo", &implied));

        assert!(layout.migrate(replacement));
        let moved = &layout.instances[0];
        assert_eq!(
            moved.instance, "photo-frame-1",
            "the id, and so the storage, stay"
        );
        assert_eq!(moved.widget, "photo");
        assert_eq!(
            moved.settings["variant"], "polaroid",
            "the implied settings win"
        );
        assert_eq!(moved.settings["shuffle"], true, "the rest is kept");
        assert_eq!(
            moved.settings["image1"],
            "https://photo.widget.mypaper/__data/photo-frame-1/a.jpg"
        );
        assert_eq!(layout.instances[1].widget, "clock");
        assert!(!layout.migrate(replacement), "nothing left to move");
    }

    #[test]
    fn parses_and_round_trips() {
        let layout = Layout::parse(LAYOUT).unwrap();
        assert_eq!(layout.instances[0].monitor, r"\\.\DISPLAY2");
        let json = serde_json::to_string(&layout).unwrap();
        assert_eq!(Layout::parse(&json).unwrap(), layout);
        assert_eq!(Layout::parse("{}").unwrap(), Layout::default());
    }

    #[test]
    fn instances_are_counted_per_widget_and_per_monitor() {
        let mut layout = Layout::default();
        layout.add("clock", "D1", [0, 0], [1, 1]);
        layout.add("clock", "D1", [1, 0], [1, 1]);
        layout.add("clock", "D2", [0, 0], [1, 1]);
        layout.add("notes", "D2", [2, 0], [1, 1]);
        assert_eq!(
            layout.counts(),
            BTreeMap::from([("clock", 3), ("notes", 1)])
        );
        let on = |device: &'static str| move |i: &Instance| i.monitor == device;
        assert_eq!(layout.count_of("clock", on("D1")), (3, 2));
        assert_eq!(layout.count_of("clock", on("D3")), (3, 0));
        assert_eq!(layout.count_of("weather", on("D1")), (0, 0));
        // Together with the manifest's rule: what an add is allowed.
        let (total, here) = layout.count_of("notes", on("D1"));
        assert!(crate::manifest::InstanceRule::PerMonitor.allows(total, here));
        assert!(!crate::manifest::InstanceRule::Single.allows(total, here));
    }

    #[test]
    fn the_overlay_switch_defaults_on_and_is_remembered() {
        assert!(Layout::parse(LAYOUT).unwrap().overlay);
        let off = Layout::parse(r#"{ "overlay": false }"#).unwrap();
        assert!(!off.overlay);
        let saved = serde_json::to_value(&off).unwrap();
        assert_eq!(saved["overlay"], false);
        assert_eq!(Layout::parse(&saved.to_string()).unwrap(), off);
        assert!(Layout::parse(r#"{ "overlay": "no" }"#).is_err());
    }

    #[test]
    fn starter_stacks_on_the_right_edge_and_skips_what_does_not_fit() {
        let widgets = [
            ("clock", [2, 1]),
            ("system-monitor", [2, 2]),
            ("now-playing", [3, 1]),
            ("notes", [2, 2]),
        ];
        let layout = Layout::starter("D1", 12, 4, &widgets);
        let placed: Vec<_> = layout
            .instances
            .iter()
            .map(|i| (i.widget.as_str(), i.cell, i.size))
            .collect();
        assert_eq!(
            placed,
            [
                ("clock", [10, 0], [2, 1]),
                ("system-monitor", [10, 1], [2, 2]),
                ("now-playing", [9, 3], [3, 1]),
            ]
        );
        assert!(Layout::starter("D1", 1, 1, &widgets).instances.is_empty());
    }

    #[test]
    fn rejects_bad_files() {
        for (bad, why) in [
            (
                LAYOUT.replace("\"version\": 1", "\"version\": 3"),
                "version",
            ),
            (LAYOUT.replace("\"cell\": 0", "\"cell\": 4"), "tiny cell"),
            (LAYOUT.replace("\"gap\": 8", "\"gap\": -1"), "negative gap"),
            (LAYOUT.replace("[2, 1]", "[0, 1]"), "zero size"),
            (
                LAYOUT.replace("\"settings\"", "\"setings\""),
                "unknown field",
            ),
        ] {
            assert!(Layout::parse(&bad).is_err(), "{why}");
        }
        let twice = LAYOUT.replace("\"widget\": \"clock\"", "\"widget\": \"clock\" }, { \"instance\": \"clock-1\", \"widget\": \"clock\", \"monitor\": \"x\", \"cell\": [0,0], \"size\": [1,1]");
        assert!(Layout::parse(&twice).is_err(), "duplicate ids");
    }

    #[test]
    fn icon_spacing_converts_twips_to_dips() {
        assert_eq!(icon_spacing_dip(Some(-1125)), 75.0); // the Windows default: 75 px at 100%
        assert_eq!(icon_spacing_dip(Some(-1440)), 96.0); // one inch
        assert_eq!(icon_spacing_dip(Some(100)), 100.0);
        assert_eq!(icon_spacing_dip(None), 96.0);
        assert_eq!(icon_spacing_dip(Some(-15)), 96.0); // 1 px: absurd
    }

    const WORK: RectF = RectF {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1040.0,
    };

    #[test]
    fn grid_fits_whole_cells_in_the_work_area() {
        let grid = Grid::new(&GridSpec::default(), [96.0, 96.0], WORK);
        assert_eq!(grid.origin, [16.0, 16.0]);
        // (1920 - 32 + 8) / 104 = 18.2 → 18 icon cells; (1040 - 32 + 8) / 104 = 9.7 → 9; each
        // split in two.
        assert_eq!((grid.cols, grid.rows), (36, 18));
        // Stretched to span the full area: the last cell ends at the right/bottom margin.
        let last = grid.rect([35, 17], [1, 1]);
        assert!((last.x + last.width - (1920.0 - 16.0)).abs() < 1e-9);
        assert!((last.y + last.height - (1040.0 - 16.0)).abs() < 1e-9);
        let fixed = Grid::new(
            &GridSpec {
                cell: 120.0,
                ..GridSpec::default()
            },
            [96.0, 96.0],
            WORK,
        );
        // A fixed nominal cell decides the count ((1888 + 8) / 128 = 14.8 → 14,
        // (1008 + 8) / 128 = 7.9 → 7); cells then stretch to fill, and split.
        assert_eq!((fixed.cols, fixed.rows), (28, 14));
        assert!(fixed.rect([0, 0], [2, 2]).width >= 120.0);
    }

    #[test]
    fn grid_follows_a_work_area_offset_and_rectangular_cells() {
        let work = RectF {
            x: 0.0,
            y: 48.0,
            ..WORK
        }; // taskbar on top
        let grid = Grid::new(&GridSpec::default(), [75.0, 90.0], work);
        assert_eq!(grid.origin, [16.0, 64.0]);
        // (1888 + 8) / 83 = 22.8 → 22 icon columns; (1008 + 8) / 98 = 10.4 → 10 rows; split.
        assert_eq!((grid.cols, grid.rows), (44, 20));
        let rect = grid.rect([1, 2], [2, 1]);
        assert!((rect.x - (16.0 + grid.cell[0] + 8.0)).abs() < 1e-9);
        assert!((rect.y - (64.0 + 2.0 * (grid.cell[1] + 8.0))).abs() < 1e-9);
        assert!((rect.width - (2.0 * grid.cell[0] + 8.0)).abs() < 1e-9);
        let icon = grid.rect([0, 0], [2, 2]);
        assert!(icon.width >= 75.0 && icon.height >= 90.0);
    }

    #[test]
    fn whole_fine_cells_fill_every_area_edge_to_edge() {
        for (width, height) in [(2560.0, 1352.0), (1920.0, 1040.0), (1366.0, 705.0), (3440.0, 1377.5)]
        {
            for icon in [[75.0, 75.0], [96.0, 96.0], [75.0, 90.0], [137.0, 101.0]] {
                let work = RectF { x: 0.0, y: 36.0, width, height };
                let grid = Grid::new(&GridSpec::default(), icon, work);
                let all = grid.rect([0, 0], [grid.cols, grid.rows]);
                let margin = GridSpec::default().margin;
                assert!((all.x - margin).abs() < 1e-9 && (all.y - (36.0 + margin)).abs() < 1e-9);
                assert!((all.x + all.width - (width - margin)).abs() < 1e-9, "{width} {icon:?}");
                assert!((all.y + all.height - (36.0 + height - margin)).abs() < 1e-9);
                // About half an icon cell: never coarser, never below a third.
                for i in 0..2 {
                    assert!(grid.cell[i] <= icon[i] / 2.0 + 8.0 && grid.cell[i] >= icon[i] / 3.0);
                }
                assert_eq!((grid.cols % 2, grid.rows % 2), (0, 0), "icon cells split in two");
            }
        }
    }

    #[test]
    fn version_one_layouts_upgrade_onto_the_same_rectangles() {
        let mut layout = Layout::parse(LAYOUT).unwrap();
        assert_eq!(layout.version, 1, "parsing alone never upgrades (Studio edits raw files)");
        let grid = Grid::new(&GridSpec::default(), [96.0, 96.0], WORK);
        // A version-1 cell was one icon cell: two fine cells and the gap between them.
        let icon = grid.cell[0] * 2.0 + grid.gap;
        let old = |cell: [u32; 2], size: [u32; 2]| RectF {
            x: grid.origin[0] + f64::from(cell[0]) * (icon + grid.gap),
            y: grid.origin[1] + f64::from(cell[1]) * (grid.cell[1] * 2.0 + 2.0 * grid.gap),
            width: f64::from(size[0]) * icon + f64::from(size[0] - 1) * grid.gap,
            height: f64::from(size[1]) * (grid.cell[1] * 2.0 + grid.gap)
                + f64::from(size[1] - 1) * grid.gap,
        };
        layout.instances[0].cell = [3, 2];
        layout.instances[0].size = [3, 1];
        let before = old([3, 2], [3, 1]);
        assert!(layout.upgrade());
        let after = &layout.instances[0];
        assert_eq!((after.cell, after.size), ([6, 4], [6, 2]));
        let now = grid.rect(after.cell, after.size);
        for (a, b) in [(before.x, now.x), (before.y, now.y), (before.width, now.width), (before.height, now.height)] {
            assert!((a - b).abs() < 1e-9, "{before:?} vs {now:?}");
        }
        assert_eq!(layout.version, VERSION);
        assert!(!layout.upgrade(), "upgrades once");
        let saved = serde_json::to_string(&layout).unwrap();
        assert_eq!(Layout::parse(&saved).unwrap(), layout);
    }

    #[test]
    fn the_upgrade_keeps_the_version_one_file_and_older_writers_cannot_undo_it() {
        let dir = crate::files::temp_dir("upgrade-backup");
        let path = dir.join("widgets.json");
        std::fs::write(&path, LAYOUT).unwrap();
        let (layout, upgraded) = Layout::load_upgrading(&path).unwrap();
        assert!(upgraded && layout.version == VERSION);
        let kept = dir.join("widgets.json.v1");
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), LAYOUT, "kept byte for byte");
        layout.save(&path).unwrap();
        // Loading again: nothing to upgrade, the backup is never replaced.
        assert!(!Layout::load_upgrading(&path).unwrap().1);
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), LAYOUT);
        // A version-1 write over the version-2 file (an older Studio) is refused.
        let old = serde_json::from_str::<Value>(r#"{ "version": 1, "instances": [] }"#).unwrap();
        assert!(Layout::save_value(&old, &path).is_err());
        assert_eq!(Layout::load(&path).unwrap(), layout, "the upgraded file stands");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_newer_layout_is_reported_as_newer_and_never_overwritten() {
        let newer = r#"{ "version": 3, "instances": [], "somethingNew": true }"#;
        assert_eq!(newer_version(newer), Some(3));
        assert_eq!(newer_version(LAYOUT), None);
        assert_eq!(newer_version("{ broken"), None);
        let error = format!("{:#}", Layout::parse(newer).unwrap_err());
        assert!(error.starts_with(NEWER), "{error}");
        let dir = crate::files::temp_dir("newer-layout");
        let path = dir.join("widgets.json");
        std::fs::write(&path, newer).unwrap();
        let error = format!("{:#}", Layout::default().save(&path).unwrap_err());
        assert!(error.starts_with(NEWER), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), newer, "left exactly as it was");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "nothing to back up: it isn't broken");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn regions_cover_widgets_in_physical_pixels() {
        let rect = RectF {
            x: 16.0,
            y: 16.0,
            width: 200.5,
            height: 96.0,
        };
        assert_eq!(rect.to_pixels(1.0), [15, 15, 218, 113]);
        assert_eq!(rect.to_pixels(1.5), [23, 23, 326, 169]);
    }

    #[test]
    fn clamping_keeps_widgets_on_the_grid() {
        let grid = Grid::new(&GridSpec::default(), [96.0, 96.0], WORK); // 36 x 18
        assert_eq!(grid.clamp([35, 17], [2, 1]), ([34, 17], [2, 1]));
        assert_eq!(grid.clamp([3, 3], [80, 80]), ([0, 0], [36, 18]));
        assert_eq!(grid.clamp([3, 3], [2, 2]), ([3, 3], [2, 2]));
    }

    #[test]
    fn placement_clamps_to_manifest_and_grid_and_resolves_settings() {
        let grid = Grid::new(&GridSpec::default(), [96.0, 96.0], WORK);
        let mut instance = Layout::parse(LAYOUT).unwrap().instances.remove(0);
        instance.cell = [50, 50];
        instance.size = [9, 9];
        let placed = place(&instance, &manifest(), &grid, &look::defaults());
        assert_eq!((placed.cell, placed.size), ([32, 16], [4, 2]));
        assert_eq!(placed.appearance["surface"], "glass");
        assert!(placed.own.is_empty());
        assert_eq!(placed.settings["format24"], Value::Bool(true));
    }

    #[test]
    fn missing_monitors_fall_back_to_the_primary() {
        let devices = [r"\\.\DISPLAY1".to_owned(), r"\\.\DISPLAY2".to_owned()];
        assert_eq!(show_on(r"\\.\DISPLAY2", &devices, 0), Some(r"\\.\DISPLAY2"));
        assert_eq!(show_on(r"\\.\DISPLAY9", &devices, 0), Some(r"\\.\DISPLAY1"));
        assert_eq!(show_on("x", &[], 0), None);
    }

    const LOOK_LAYOUT: &str = r##"{
        "version": 1,
        "appearance": { "surface": "glass", "tint": "#202020", "accent": "auto", "opacity": 0.7 },
        "instances": [
            { "instance": "clock-1", "widget": "clock", "monitor": "m", "cell": [0, 0], "size": [2, 1],
              "appearance": { "text": "dark", "scale": 1.25 } },
            { "instance": "clock-2", "widget": "clock", "monitor": "m", "cell": [2, 0], "size": [2, 1] }
        ]
    }"##;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn appearance_layers_parse_validate_and_save_only_what_is_set() {
        let layout = Layout::parse(LOOK_LAYOUT).unwrap();
        assert_eq!(layout.appearance.opacity, Some(0.7));
        assert_eq!(layout.instances[0].appearance.scale, Some(1.25));
        assert!(layout.instances[1].appearance == Appearance::default());

        // Written back: no nulls, no empty layers, and it reads back the same.
        let json: Value = serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
        assert_eq!(json["appearance"]["opacity"], 0.7);
        assert!(json["appearance"].get("blur").is_none());
        assert!(json["instances"][1].get("appearance").is_none());
        assert_eq!(Layout::parse(&json.to_string()).unwrap(), layout);
        let empty = serde_json::to_string(&Layout::default()).unwrap();
        assert!(!empty.contains("appearance"));

        for (bad, why) in [
            (LOOK_LAYOUT.replace("0.7", "1.7"), "opacity range"),
            (LOOK_LAYOUT.replace("1.25", "3"), "instance scale range"),
            (LOOK_LAYOUT.replace("#202020", "gray"), "color"),
            (
                LOOK_LAYOUT.replace("\"surface\"", "\"surfce\""),
                "unknown field",
            ),
        ] {
            assert!(Layout::parse(&bad).is_err(), "{why}");
        }
    }

    #[test]
    fn appearance_edits_set_clear_and_stay_valid() {
        let mut layout = Layout::parse(LOOK_LAYOUT).unwrap();

        layout
            .set_appearance(
                None,
                &object(serde_json::json!({ "blur": 40, "opacity": null })),
            )
            .unwrap();
        assert_eq!(layout.appearance.blur, Some(40.0));
        assert_eq!(
            layout.appearance.opacity, None,
            "cleared: the theme shows through"
        );
        let tint = layout.appearance.tint.map(|c| c.hex());
        assert_eq!(tint.as_deref(), Some("#202020"));

        layout
            .set_appearance(Some("clock-2"), &object(serde_json::json!({ "radius": 4 })))
            .unwrap();
        assert_eq!(layout.get("clock-2").unwrap().appearance.radius, Some(4.0));
        assert_eq!(layout.get("clock-1").unwrap().appearance.radius, None);

        // A refused edit changes nothing.
        let before = layout.clone();
        let too_big = object(serde_json::json!({ "scale": 9 }));
        assert!(layout.set_appearance(Some("clock-2"), &too_big).is_err());
        assert!(layout.set_appearance(Some("nope"), &Map::new()).is_err());
        assert_eq!(layout, before);

        // Duplicates keep their look.
        let copy = layout.duplicate("clock-2", [6, 0]).unwrap();
        assert_eq!(layout.get(&copy).unwrap().appearance.radius, Some(4.0));
    }

    #[test]
    fn edits() {
        let mut layout = Layout::parse(LAYOUT).unwrap();
        let added = layout.add("clock", r"\\.\DISPLAY1", [3, 0], [2, 1]);
        assert_eq!(added, "clock-2");
        let copy = layout.duplicate("clock-1", [5, 0]).unwrap();
        assert_eq!(copy, "clock-3");
        assert_eq!(
            layout.get("clock-3").unwrap().settings["format24"],
            Value::Bool(true)
        );
        layout.remove("clock-2").unwrap();
        assert_eq!(layout.new_id("clock"), "clock-2");
        layout.apply(&[("clock-1".into(), [1, 1], [3, 2])]).unwrap();
        assert_eq!(layout.get("clock-1").unwrap().cell, [1, 1]);
        assert!(layout.apply(&[("nope".into(), [0, 0], [1, 1])]).is_err());
        assert!(layout.remove("nope").is_err());
        let values = serde_json::json!({ "format24": false });
        layout
            .set_settings("clock-1", values.as_object().unwrap().clone())
            .unwrap();
        assert_eq!(
            layout.get("clock-1").unwrap().settings["format24"],
            Value::Bool(false)
        );
    }
}
