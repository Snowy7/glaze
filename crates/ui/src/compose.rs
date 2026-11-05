//! DirectComposition sprites over a surface's window: each bitmap is uploaded once
//! ([`Compositor::sprite`]), then DWM places and scales it on every frame
//! ([`Compositor::show`]): no redraw and no readback in this process.
//!
//! Backed by a WARP (software) Direct3D 11 device, so no GPU vendor driver loads here (plan
//! Rule 1, `tests/compose_no_gpu_driver.rs`); DWM composites the uploaded surfaces on the GPU.
use crate::{Image, Rect};
use anyhow::{Context, Result};
use windows::Win32::{
    Foundation::{HMODULE, HWND, POINT},
    Graphics::{
        Direct3D::{D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0},
        Direct3D11::{
            D3D11_BOX, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
            ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
        },
        DirectComposition::{
            DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR, DCompositionCreateDevice,
            IDCompositionDevice, IDCompositionEffectGroup, IDCompositionSurface,
            IDCompositionTarget, IDCompositionVisual,
        },
        Dxgi::{
            Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM},
            IDXGIDevice,
        },
    },
};
use windows_core::Interface;

/// One window's composition tree: a root visual, above the window's own (layered) pixels, and
/// one child visual per shown sprite, back to front.
pub struct Compositor {
    context: ID3D11DeviceContext,
    device: IDCompositionDevice,
    _target: IDCompositionTarget,
    root: IDCompositionVisual,
    slots: Vec<Slot>,
    _d3d: ID3D11Device,
}

/// An uploaded bitmap; one sprite may be shown in several places at once.
pub struct Sprite {
    surface: IDCompositionSurface,
    size: (u32, u32),
}

/// A visual of the tree and what it shows now.
struct Slot {
    visual: IDCompositionVisual,
    effect: IDCompositionEffectGroup,
    content: Option<IDCompositionSurface>,
    rect: Rect,
    opacity: f32,
}

impl Compositor {
    /// Composes over `hwnd` (topmost: above its layered-window pixels).
    pub fn new(hwnd: HWND) -> Result<Self> {
        // SAFETY: documented creation calls; every out-pointer outlives its call.
        unsafe {
            let mut d3d = None;
            let mut context = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                Some(&mut context),
            )
            .context("WARP Direct3D 11 device")?;
            let d3d = d3d.context("no WARP device")?;
            let context = context.context("no WARP device context")?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let device: IDCompositionDevice =
                DCompositionCreateDevice(&dxgi).context("DirectComposition device")?;
            let target = device
                .CreateTargetForHwnd(hwnd, true)
                .context("DirectComposition target")?;
            let root = device.CreateVisual()?;
            target.SetRoot(&root)?;
            Ok(Self {
                context,
                device,
                _target: target,
                root,
                slots: vec![],
                _d3d: d3d,
            })
        }
    }

    /// Uploads `image` (premultiplied BGRA) into a new sprite, not yet shown.
    pub fn sprite(&self, image: &Image) -> Result<Sprite> {
        anyhow::ensure!(
            image.width > 0
                && image.height > 0
                && image.pixels.len() == (image.width * image.height * 4) as usize,
            "sprite pixels do not match {}x{}",
            image.width,
            image.height
        );
        let (w, h) = (image.width, image.height);
        // SAFETY: the surface is drawn between BeginDraw and EndDraw; the copied box lies inside
        // the update rect DComp returned, and `pixels` holds `w * h * 4` bytes (checked above).
        unsafe {
            let surface = self
                .device
                .CreateSurface(w, h, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_PREMULTIPLIED)
                .context("DirectComposition surface")?;
            let mut at = POINT::default();
            let texture: ID3D11Texture2D = surface.BeginDraw(None, &mut at)?;
            let area = D3D11_BOX {
                left: at.x as u32,
                top: at.y as u32,
                front: 0,
                right: at.x as u32 + w,
                bottom: at.y as u32 + h,
                back: 1,
            };
            self.context.UpdateSubresource(
                &texture,
                0,
                Some(&area),
                image.pixels.as_ptr().cast(),
                w * 4,
                0,
            );
            surface.EndDraw()?;
            Ok(Sprite {
                surface,
                size: (w, h),
            })
        }
    }

    /// Shows exactly `sprites`, back to front, each over its rect (window px; its bitmap
    /// stretched to fit) at its opacity, and hands the changes to DWM in one commit. Only what
    /// differs from the shown frame is sent; an unchanged frame commits nothing.
    pub fn show(&mut self, sprites: &[(&Sprite, Rect, f32)]) -> Result<()> {
        let mut changed = false;
        // SAFETY: documented calls on visuals and surfaces of this device; matrices outlive
        // their calls.
        unsafe {
            while self.slots.len() > sprites.len() {
                let slot = self.slots.pop().expect("longer than sprites");
                self.root.RemoveVisual(&slot.visual)?;
                changed = true;
            }
            while self.slots.len() < sprites.len() {
                let visual = self.device.CreateVisual()?;
                visual.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR)?;
                let effect = self.device.CreateEffectGroup()?;
                visual.SetEffect(&effect)?;
                // Above every other child: slots stay in back-to-front order.
                self.root.AddVisual(&visual, true, None)?;
                self.slots.push(Slot {
                    visual,
                    effect,
                    content: None,
                    rect: Rect::default(),
                    opacity: 1.0,
                });
                changed = true;
            }
            for (slot, &(sprite, rect, opacity)) in self.slots.iter_mut().zip(sprites) {
                if slot.content.as_ref() != Some(&sprite.surface) {
                    slot.visual.SetContent(&sprite.surface)?;
                    slot.content = Some(sprite.surface.clone());
                    slot.rect = Rect::default();
                    changed = true;
                }
                if slot.rect != rect {
                    slot.visual.SetTransform2(&windows_numerics::Matrix3x2 {
                        M11: rect.width() / sprite.size.0 as f32,
                        M12: 0.0,
                        M21: 0.0,
                        M22: rect.height() / sprite.size.1 as f32,
                        M31: rect.left,
                        M32: rect.top,
                    })?;
                    slot.rect = rect;
                    changed = true;
                }
                let opacity = opacity.clamp(0.0, 1.0);
                if slot.opacity != opacity {
                    slot.effect.SetOpacity2(opacity)?;
                    slot.opacity = opacity;
                    changed = true;
                }
            }
            if changed {
                self.device.Commit().context("DirectComposition commit")?;
            }
        }
        Ok(())
    }
}
