// Glass overlays (overlay/glass.rs): counts the pixels where a fresh crop of the backdrop differs
// from the one the glass shows. The capture reports dirty rects wherever an excluded window
// (our overlay, the shell's popup) repaints or moves, though the pixels behind it are unchanged;
// a count of 0 means the frame is skipped, which is what stops the glass redrawing itself.

@group(0) @binding(0) var fresh: texture_2d<f32>;
@group(0) @binding(1) var shown: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> differ: atomic<u32>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(fresh);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    if any(textureLoad(fresh, id.xy, 0) != textureLoad(shown, id.xy, 0)) {
        atomicAdd(&differ, 1u);
    }
}
