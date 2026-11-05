//! Which graphics adapter drives the screens. The engine renders there by default
//! (`engine.gpu` = `auto`): a frame drawn on another GPU is copied across to the one scanning it
//! out, which costs both GPUs time and the engine memory (docs/glass.md, GPU default).

use windows::Win32::Foundation::LUID;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

/// An adapter with screens attached to the desktop.
#[derive(Debug, Clone)]
pub struct DisplayAdapter {
    pub luid: LUID,
    pub name: String,
    /// Desktop screens it drives.
    pub screens: u32,
}

/// The adapter that drives the most desktop screens; on a tie, the primary screen's. `None`
/// when DXGI lists no attached outputs (some remote sessions). Reads descriptions only: no
/// device is created, so no vendor driver loads.
pub fn display_adapter() -> Option<DisplayAdapter> {
    let mut best: Option<(u32, bool, DisplayAdapter)> = None;
    // SAFETY: DXGI factory, adapter and output descriptions; nothing outlives the call.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            let (mut screens, mut primary) = (0, false);
            let mut j = 0;
            while let Ok(output) = adapter.EnumOutputs(j) {
                j += 1;
                if let Ok(out) = output.GetDesc()
                    && out.AttachedToDesktop.as_bool()
                {
                    screens += 1;
                    // The primary screen's top-left is the desktop's origin.
                    let r = out.DesktopCoordinates;
                    primary |= r.left == 0 && r.top == 0;
                }
            }
            if screens > 0 && best.as_ref().is_none_or(|b| (screens, primary) > (b.0, b.1)) {
                let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
                let name = String::from_utf16_lossy(&desc.Description[..len]);
                best = Some((screens, primary, DisplayAdapter { luid: desc.AdapterLuid, name, screens }));
            }
        }
    }
    best.map(|b| b.2)
}

#[cfg(test)]
mod tests {
    /// On any machine with a screen, some adapter drives it.
    #[test]
    #[ignore = "needs a desktop session"]
    fn finds_the_screens_adapter() {
        let adapter = super::display_adapter().expect("an adapter with screens");
        eprintln!("{adapter:?}");
        assert!(adapter.screens > 0);
    }
}
