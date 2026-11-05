//! DirectComposition sprites over a surface's window: each bitmap is uploaded once, then DWM
//! moves and scales it on every frame ([`Compositor::place`] + [`Compositor::commit`]): no
//! redraw and no readback in this process.
//!
//! Backed by a WARP (software) Direct3D 11 device, so no GPU vendor driver loads here (plan
//! Rule 1, `tests/no_gpu_driver.rs`); DWM composites the uploaded surfaces on the GPU.
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
            IDCompositionDevice, IDCompositionEffectGroup, IDCompositionTarget,
            IDCompositionVisual,
        },
        Dxgi::{
            Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM},
            IDXGIDevice,
        },
    },
};
use windows_core::Interface;

/// One window's composition tree: a root visual, above the window's own (layered) pixels.
pub struct Compositor {
    context: ID3D11DeviceContext,
    device: IDCompositionDevice,
    _target: IDCompositionTarget,
    root: IDCompositionVisual,
    _d3d: ID3D11Device,
}

/// A bitmap DWM shows at any place, scale and opacity.
pub struct Sprite {
    visual: IDCompositionVisual,
    effect: IDCompositionEffectGroup,
    size: (u32, u32),
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
                _d3d: d3d,
            })
        }
    }

    /// Uploads `image` (premultiplied BGRA) into a new sprite, not yet shown.
    pub fn sprite(&self, image: &Image) -> Result<Sprite> {
        let (w, h) = (image.width.max(1), image.height.max(1));
        anyhow::ensure!(
            image.pixels.len() == (image.width * image.height * 4) as usize,
            "sprite pixels do not match {}x{}",
            image.width,
            image.height
        );
        // SAFETY: the surface is drawn between BeginDraw and EndDraw; the copied box lies inside
        // the update rect DComp returned, and `pixels` holds `w * h * 4` bytes (checked above).
        unsafe {
            let surface = self.device.CreateSurface(
                w,
                h,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_ALPHA_MODE_PREMULTIPLIED,
            )?;
            let mut at = POINT::default();
            let texture: ID3D11Texture2D = surface.BeginDraw(None, &mut at)?;
            if image.width > 0 && image.height > 0 {
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
            }
            surface.EndDraw()?;
            let visual = self.device.CreateVisual()?;
            visual.SetContent(&surface)?;
            visual.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR)?;
            let effect = self.device.CreateEffectGroup()?;
            visual.SetEffect(&effect)?;
            Ok(Sprite {
                visual,
                effect,
                size: (w, h),
            })
        }
    }

    /// Shows exactly `sprites`, back to front (call when the set changes, not per frame).
    pub fn show(&self, sprites: &[&Sprite]) -> Result<()> {
        // SAFETY: documented calls on visuals of this device.
        unsafe {
            self.root.RemoveAllVisuals()?;
            for sprite in sprites {
                self.root.AddVisual(&sprite.visual, true, None)?;
            }
        }
        Ok(())
    }

    /// Puts `sprite` over `rect` (window px; its bitmap stretched to fit) at `opacity`.
    pub fn place(&self, sprite: &Sprite, rect: Rect, opacity: f32) -> Result<()> {
        let matrix = windows_numerics::Matrix3x2 {
            M11: rect.width() / sprite.size.0 as f32,
            M12: 0.0,
            M21: 0.0,
            M22: rect.height() / sprite.size.1 as f32,
            M31: rect.left,
            M32: rect.top,
        };
        // SAFETY: documented calls; the matrix outlives the call.
        unsafe {
            sprite.visual.SetTransform2(&matrix)?;
            sprite.effect.SetOpacity2(opacity.clamp(0.0, 1.0))?;
        }
        Ok(())
    }

    /// Hands this frame's changes to DWM, atomically.
    pub fn commit(&self) -> Result<()> {
        // SAFETY: a documented call on this device.
        unsafe { self.device.Commit() }.context("DirectComposition commit")
    }
}
