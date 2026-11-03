//! `widgets.json` (where every widget instance sits) and the grid it sits on
//! (docs/widgets.md, "Layout").
//!
//! Pure: no Win32. Lengths are DIPs (CSS pixels) relative to the monitor's
//! top-left corner unless a name says otherwise.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::manifest::{WidgetManifest, valid_id};

pub const VERSION: u32 = 1;

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
}

fn version() -> u32 {
    VERSION
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            version: VERSION,
            grid: GridSpec::default(),
            instances: Vec::new(),
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
        let layout: Self = serde_json::from_str(json)?;
        layout.validate()?;
        Ok(layout)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "version {} isn't supported (expected {VERSION})",
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
        }
        Ok(())
    }

    /// Writes atomically, so watchers never see a half-written file.
    pub fn save(&self, path: &Path) -> Result<()> {
        crate::files::write_atomic(
            path,
            (serde_json::to_string_pretty(self)? + "\n").as_bytes(),
        )
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

    /// Merges setting values into an instance's saved settings.
    pub fn set_settings(&mut self, instance: &str, values: Map<String, Value>) -> Result<()> {
        self.get_mut(instance)?.settings.extend(values);
        Ok(())
    }
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
    /// The grid for a work area: `spec.cell` (or `icon_cell` when it's 0),
    /// starting at the work area plus the margin, with as many whole cells
    /// as fit (at least one).
    pub fn new(spec: &GridSpec, icon_cell: [f64; 2], work: RectF) -> Self {
        let cell = if spec.cell > 0.0 {
            [spec.cell; 2]
        } else {
            icon_cell
        };
        let fit = |length: f64, cell: f64| {
            let room = length - 2.0 * spec.margin + spec.gap;
            ((room / (cell + spec.gap)).floor() as u32).max(1)
        };
        Self {
            origin: [work.x + spec.margin, work.y + spec.margin],
            cell,
            gap: spec.gap,
            cols: fit(work.width, cell[0]),
            rows: fit(work.height, cell[1]),
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
}

/// Places an instance on `grid`: the size clamped to the manifest and the
/// grid, the cell to the grid, and its settings resolved.
pub fn place(instance: &Instance, manifest: &WidgetManifest, grid: &Grid) -> Placed {
    let (cell, size) = grid.clamp(instance.cell, manifest.clamp_size(instance.size));
    Placed {
        instance: instance.instance.clone(),
        widget: instance.widget.clone(),
        cell,
        size,
        settings: manifest.resolve_settings(&instance.settings),
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
    fn parses_and_round_trips() {
        let layout = Layout::parse(LAYOUT).unwrap();
        assert_eq!(layout.instances[0].monitor, r"\\.\DISPLAY2");
        let json = serde_json::to_string(&layout).unwrap();
        assert_eq!(Layout::parse(&json).unwrap(), layout);
        assert_eq!(Layout::parse("{}").unwrap(), Layout::default());
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
                LAYOUT.replace("\"version\": 1", "\"version\": 2"),
                "version",
            ),
            (LAYOUT.replace("\"cell\": 0", "\"cell\": 4"), "tiny cell"),
            (LAYOUT.replace("\"gap\": 8", "\"gap\": -1"), "negative gap"),
            (LAYOUT.replace("clock-1", "Clock 1"), "instance id"),
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
        // (1920 - 32 + 8) / 104 = 18.2 → 18; (1040 - 32 + 8) / 104 = 9.7 → 9
        assert_eq!((grid.cols, grid.rows), (18, 9));
        let fixed = Grid::new(
            &GridSpec {
                cell: 120.0,
                ..GridSpec::default()
            },
            [96.0, 96.0],
            WORK,
        );
        assert_eq!(fixed.cell, [120.0, 120.0]);
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
        assert_eq!(
            grid.rect([1, 2], [2, 1]),
            RectF {
                x: 16.0 + 83.0,
                y: 64.0 + 2.0 * 98.0,
                width: 158.0,
                height: 90.0
            }
        );
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
        let grid = Grid::new(&GridSpec::default(), [96.0, 96.0], WORK); // 18 x 9
        assert_eq!(grid.clamp([17, 8], [2, 1]), ([16, 8], [2, 1]));
        assert_eq!(grid.clamp([3, 3], [40, 40]), ([0, 0], [18, 9]));
        assert_eq!(grid.clamp([3, 3], [2, 2]), ([3, 3], [2, 2]));
    }

    #[test]
    fn placement_clamps_to_manifest_and_grid_and_resolves_settings() {
        let grid = Grid::new(&GridSpec::default(), [96.0, 96.0], WORK);
        let mut instance = Layout::parse(LAYOUT).unwrap().instances.remove(0);
        instance.cell = [50, 50];
        instance.size = [9, 9];
        let placed = place(&instance, &manifest(), &grid);
        assert_eq!((placed.cell, placed.size), ([14, 7], [4, 2]));
        assert_eq!(placed.settings["format24"], Value::Bool(true));
    }

    #[test]
    fn missing_monitors_fall_back_to_the_primary() {
        let devices = [r"\\.\DISPLAY1".to_owned(), r"\\.\DISPLAY2".to_owned()];
        assert_eq!(show_on(r"\\.\DISPLAY2", &devices, 0), Some(r"\\.\DISPLAY2"));
        assert_eq!(show_on(r"\\.\DISPLAY9", &devices, 0), Some(r"\\.\DISPLAY1"));
        assert_eq!(show_on("x", &[], 0), None);
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
