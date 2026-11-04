//! Live blur (docs/bars.md, "Live blur"): what's behind a bar's window, other windows included,
//! blurred by Windows and drawn under the page.
//!
//! A WebView can't see behind its window, so `backdrop-filter` only blurs the page's own pixels.
//! DWM can: each bar window opts into host backdrop brushes (`DWMWA_USE_HOSTBACKDROPBRUSH`,
//! Windows 11) and gets a composition target *under* its child windows, so under the transparent
//! WebView. The page reports where its glass is (`glass`: rounded rects with an opacity and a
//! material, DIPs from the window's top-left, every frame while one moves); each becomes a sprite
//! of the host backdrop (DWM's own acrylic blur of what's behind), through an optional extra
//! Gaussian blur and a color matrix (saturation, brightness), clipped to its anti-aliased rounded
//! rect. The page draws the tint, rim, sheen and grain over it.
//!
//! Nothing here polls or animates: DWM redraws a sprite only when what's behind it changes, and a
//! pane changes only when the page says so. Without Windows 11, with transparency effects off, or
//! in performance mode, the page is told (`mode.blur`) and draws an opaque tint instead.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::Deserialize;
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{
    IGraphicsEffect, IGraphicsEffect_Impl, IGraphicsEffectSource, IGraphicsEffectSource_Impl,
};
use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionBrush, CompositionEffectSourceParameter, CompositionRoundedRectangleGeometry,
    Compositor, ContainerVisual, SpriteVisual,
};
use windows::Win32::Foundation::{E_INVALIDARG, HWND};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    GRAPHICS_EFFECT_PROPERTY_MAPPING, IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl,
};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows::core::{BOOL, GUID, HSTRING, Interface, PCWSTR, implement, w};
use windows_numerics::{Vector2, Vector3};

/// The most panes one bar may report (its surfaces plus an open flyout).
pub const MAX_PANES: usize = 32;

/// A piece of glass the page draws, as the `glass` request sends it (DIPs).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pane {
    /// `[x, y, width, height]` from the window's top-left.
    pub rect: [f64; 4],
    #[serde(default)]
    pub radius: f64,
    #[serde(default = "one")]
    pub opacity: f64,
    #[serde(default = "one")]
    pub saturation: f64,
    #[serde(default = "one")]
    pub brightness: f64,
    /// Extra blur over Windows' own, DIPs.
    #[serde(default)]
    pub blur: f64,
}

fn one() -> f64 {
    1.0
}

impl Pane {
    /// Ranges a page could get wrong; the host never trusts them blindly.
    pub fn valid(&self) -> bool {
        let [x, y, w, h] = self.rect;
        [x, y, w, h, self.radius, self.opacity, self.saturation, self.brightness, self.blur]
            .iter()
            .all(|v| v.is_finite())
            && w >= 0.0
            && h >= 0.0
            && (0.0..=1.0).contains(&self.opacity)
            && (0.0..=3.0).contains(&self.saturation)
            && (0.0..=2.0).contains(&self.brightness)
            && (0.0..=64.0).contains(&self.blur)
    }

    /// The material, rounded so near-equal panes share one brush.
    fn material(&self) -> Material {
        let q = |v: f64, steps: f64| (v * steps).round() as u16;
        Material {
            saturation: q(self.saturation, 50.0),
            brightness: q(self.brightness, 50.0),
            blur: q(self.blur, 2.0),
        }
    }
}

/// A brush's parameters, quantized (saturation and brightness in 1/50ths, blur in half DIPs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Material {
    saturation: u16,
    brightness: u16,
    blur: u16,
}

/// Is "Transparency effects" on in Settings (`EnableTransparency`)? Live blur follows it.
pub fn transparency_on() -> bool {
    let mut value = 1u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: reads one DWORD into `value`, whose size is `size`.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("EnableTransparency"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    read.is_err() || value != 0
}

/// The compositor every bar window's live blur shares (one per UI thread), and the brushes made
/// so far, by material.
pub struct LiveBlur {
    _queue: DispatcherQueueController,
    compositor: Compositor,
    brushes: HashMap<Material, CompositionBrush>,
}

impl LiveBlur {
    /// Starts the compositor on this thread (its dispatcher queue runs on our message loop).
    pub fn new() -> Result<Self> {
        // SAFETY: creates a dispatcher queue for the calling (UI) thread, which pumps messages.
        let queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            })
        }
        .context("CreateDispatcherQueueController")?;
        let compositor = Compositor::new().context("Compositor")?;
        Ok(Self {
            _queue: queue,
            compositor,
            brushes: HashMap::new(),
        })
    }

    /// A composition target under `hwnd`'s children (the WebView), or why there can't be one
    /// (before Windows 11, `DWMWA_USE_HOSTBACKDROPBRUSH` is refused).
    pub fn attach(&self, hwnd: HWND) -> Result<Target> {
        let on = BOOL(1);
        // SAFETY: our own window; the attribute is a BOOL.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                (&raw const on).cast(),
                size_of::<BOOL>() as u32,
            )
        }
        .context("DWMWA_USE_HOSTBACKDROPBRUSH (Windows 11 only)")?;
        let interop: ICompositorDesktopInterop = self.compositor.cast()?;
        // SAFETY: our own window; `false` puts the target under its child windows.
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, false) }
            .context("CreateDesktopWindowTarget")?;
        let root = self.compositor.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        target.SetRoot(&root)?;
        Ok(Target {
            _target: target,
            root,
            sprites: Vec::new(),
            shown: Vec::new(),
        })
    }

    /// The brush for `material`: the host backdrop, optionally blurred more, through a color
    /// matrix. Made once per material; a material the effect pipeline refuses falls back to the
    /// plain backdrop (logged once).
    fn brush(&mut self, material: Material) -> Result<CompositionBrush> {
        if let Some(brush) = self.brushes.get(&material) {
            return Ok(brush.clone());
        }
        let brush = self.effect_brush(material).or_else(|e| {
            log::warn!("live blur: no effect brush for {material:?} ({e:#}); plain backdrop");
            Ok::<_, anyhow::Error>(self.compositor.CreateHostBackdropBrush()?.cast()?)
        })?;
        self.brushes.insert(material, brush.clone());
        Ok(brush)
    }

    fn effect_brush(&self, material: Material) -> Result<CompositionBrush> {
        let source: IGraphicsEffectSource =
            CompositionEffectSourceParameter::Create(&HSTRING::from(BACKDROP))?.cast()?;
        let blurred: IGraphicsEffectSource = if material.blur > 0 {
            let deviation = f32::from(material.blur) / 2.0;
            Effect::new(
                GAUSSIAN_BLUR,
                vec![
                    PropertyValue::CreateSingle(deviation)?.cast()?,
                    PropertyValue::CreateUInt32(2)?.cast()?, // quality
                    PropertyValue::CreateUInt32(1)?.cast()?, // hard edges: no dark rim
                ],
                source,
            )
            .into()
        } else {
            source
        };
        let matrix = color_matrix(
            f32::from(material.saturation) / 50.0,
            f32::from(material.brightness) / 50.0,
        );
        let effect: IGraphicsEffect = Effect::new(
            COLOR_MATRIX,
            vec![
                PropertyValue::CreateSingleArray(&matrix)?.cast()?,
                PropertyValue::CreateUInt32(1)?.cast()?, // premultiplied alpha
                PropertyValue::CreateBoolean(false)?.cast()?,
            ],
            blurred,
        )
        .into();
        let factory = self.compositor.CreateEffectFactory(&effect)?;
        let brush = factory.CreateBrush()?;
        brush.SetSourceParameter(
            &HSTRING::from(BACKDROP),
            &self.compositor.CreateHostBackdropBrush()?,
        )?;
        Ok(brush.cast()?)
    }
}

const BACKDROP: &str = "backdrop";
/// CLSID_D2D1ColorMatrix.
const COLOR_MATRIX: GUID = GUID::from_u128(0x921f03d6_641c_47df_852d_b4bb6153ae11);
/// CLSID_D2D1GaussianBlur.
const GAUSSIAN_BLUR: GUID = GUID::from_u128(0x1feb6d69_2fe6_4ac9_8c58_1d7f93e7a6a5);

/// CSS `saturate(s)` then `brightness(b)` as a D2D 5×4 color matrix (a row vector times the
/// matrix; rows are the input channels, the last row the offsets).
pub fn color_matrix(s: f32, b: f32) -> [f32; 20] {
    let (r, g, bl) = (0.213, 0.715, 0.072);
    let rgb = [
        [r + (1.0 - r) * s, r - r * s, r - r * s],
        [g - g * s, g + (1.0 - g) * s, g - g * s],
        [bl - bl * s, bl - bl * s, bl + (1.0 - bl) * s],
    ];
    let mut out = [0.0; 20];
    for (i, row) in rgb.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            out[i * 4 + j] = v * b;
        }
    }
    out[15] = 1.0; // alpha in → alpha out
    out
}

/// A Direct2D effect described to Windows.UI.Composition (what Win2D's effect classes are): its
/// CLSID, its properties by index, one source.
#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct Effect {
    id: GUID,
    properties: Vec<IPropertyValue>,
    source: IGraphicsEffectSource,
}

impl Effect {
    fn new(id: GUID, properties: Vec<IPropertyValue>, source: IGraphicsEffectSource) -> Self {
        Self {
            id,
            properties,
            source,
        }
    }
}

impl IGraphicsEffect_Impl for Effect_Impl {
    fn Name(&self) -> windows::core::Result<HSTRING> {
        Ok(HSTRING::new())
    }
    fn SetName(&self, _: &HSTRING) -> windows::core::Result<()> {
        Ok(())
    }
}

impl IGraphicsEffectSource_Impl for Effect_Impl {}

impl IGraphicsEffectD2D1Interop_Impl for Effect_Impl {
    fn GetEffectId(&self) -> windows::core::Result<GUID> {
        Ok(self.id)
    }
    /// No property is animated by name.
    fn GetNamedPropertyMapping(
        &self,
        _: &PCWSTR,
        _: *mut u32,
        _: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> windows::core::Result<()> {
        Err(E_INVALIDARG.into())
    }
    fn GetPropertyCount(&self) -> windows::core::Result<u32> {
        Ok(self.properties.len() as u32)
    }
    fn GetProperty(&self, index: u32) -> windows::core::Result<IPropertyValue> {
        self.properties
            .get(index as usize)
            .cloned()
            .ok_or_else(|| E_INVALIDARG.into())
    }
    fn GetSource(&self, index: u32) -> windows::core::Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(E_INVALIDARG.into())
        }
    }
    fn GetSourceCount(&self) -> windows::core::Result<u32> {
        Ok(1)
    }
}

/// One bar window's live blur: a visual tree under the WebView, one sprite per pane.
pub struct Target {
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    sprites: Vec<(SpriteVisual, CompositionRoundedRectangleGeometry)>,
    /// What the sprites show now (px), so an unchanged frame changes nothing.
    shown: Vec<(Pane, Material)>,
}

impl Target {
    /// Shows `panes` (DIPs) at `scale`: one sprite each, made or dropped as the count changes.
    pub fn set(&mut self, blur: &mut LiveBlur, panes: &[Pane], scale: f64) -> Result<()> {
        let px: Vec<(Pane, Material)> = panes
            .iter()
            .filter(|pane| pane.valid())
            .take(MAX_PANES)
            .map(|pane| {
                let [x, y, w, h] = pane.rect;
                let scaled = Pane {
                    rect: [x * scale, y * scale, w * scale, h * scale],
                    radius: pane.radius * scale,
                    blur: pane.blur * scale,
                    ..pane.clone()
                };
                (scaled, pane.material())
            })
            .collect();
        if px == self.shown {
            return Ok(());
        }
        let children = self.root.Children()?;
        while self.sprites.len() > px.len() {
            if let Some((sprite, _)) = self.sprites.pop() {
                children.Remove(&sprite)?;
            }
        }
        for (index, (pane, material)) in px.iter().enumerate() {
            if index == self.sprites.len() {
                let sprite = blur.compositor.CreateSpriteVisual()?;
                let shape = blur.compositor.CreateRoundedRectangleGeometry()?;
                sprite.SetClip(&blur.compositor.CreateGeometricClipWithGeometry(&shape)?)?;
                children.InsertAtTop(&sprite)?;
                self.sprites.push((sprite, shape));
            }
            let (sprite, shape) = &self.sprites[index];
            if self.shown.get(index).map(|(_, m)| m) != Some(material) {
                sprite.SetBrush(&blur.brush(*material)?)?;
            }
            let [x, y, w, h] = pane.rect.map(|v| v as f32);
            let size = Vector2 { X: w, Y: h };
            let radius = (pane.radius as f32).min(w / 2.0).min(h / 2.0).max(0.0);
            sprite.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
            sprite.SetSize(size)?;
            sprite.SetOpacity(pane.opacity as f32)?;
            shape.SetSize(size)?;
            shape.SetCornerRadius(Vector2 {
                X: radius,
                Y: radius,
            })?;
        }
        self.shown = px;
        Ok(())
    }

    /// Drops every pane (the page reloaded, or live blur went off).
    pub fn clear(&mut self, blur: &mut LiveBlur) {
        if let Err(e) = self.set(blur, &[], 1.0) {
            log::warn!("live blur: {e:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matrix_is_identity_at_one_and_grey_at_zero() {
        let identity = color_matrix(1.0, 1.0);
        for i in 0..4 {
            for j in 0..4 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((identity[i * 4 + j] - want).abs() < 1e-6, "[{i}][{j}]");
            }
        }
        // Saturation 0: every output channel is the same luminance mix.
        let grey = color_matrix(0.0, 1.0);
        for i in 0..3 {
            assert!((grey[i * 4] - grey[i * 4 + 1]).abs() < 1e-6);
            assert!((grey[i * 4 + 1] - grey[i * 4 + 2]).abs() < 1e-6);
        }
        assert!((color_matrix(1.0, 0.5)[0] - 0.5).abs() < 1e-6, "brightness scales");
    }

    #[test]
    fn panes_are_checked_and_share_materials() {
        let pane: Pane = serde_json::from_str(r#"{ "rect": [0, 0, 100, 40], "radius": 8 }"#).unwrap();
        assert!(pane.valid());
        assert_eq!((pane.opacity, pane.saturation), (1.0, 1.0));
        let near = Pane {
            saturation: 1.004,
            ..pane.clone()
        };
        assert_eq!(pane.material(), near.material());
        for bad in [
            Pane { opacity: 2.0, ..pane.clone() },
            Pane { rect: [0.0, 0.0, -1.0, 4.0], ..pane.clone() },
            Pane { saturation: f64::NAN, ..pane.clone() },
        ] {
            assert!(!bad.valid(), "{bad:?}");
        }
        assert!(serde_json::from_str::<Pane>(r#"{ "rect": [0, 0, 1, 1], "tint": 1 }"#).is_err());
    }
}
