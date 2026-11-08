//! Load-time mip generation. Color mips average linear, alpha-weighted light;
//! data maps remain linear. Only material-referenced color/data variants upload.
use super::load::{Texture, TextureInfo};
use crate::gpu::Gpu;

pub fn upload(gpu: &Gpu, image: &Texture, srgb: bool) -> wgpu::TextureView {
    let format = if srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    let levels = image.size.0.max(image.size.1).ilog2() + 1;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene mipmapped texture"),
        size: wgpu::Extent3d {
            width: image.size.0,
            height: image.size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut pixels = image.rgba.clone();
    let (mut width, mut height) = image.size;
    for mip_level in 0..levels {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        if mip_level + 1 < levels {
            pixels = downsample(&pixels, width, height, srgb);
            width = (width / 2).max(1);
            height = (height / 2).max(1);
        }
    }
    texture.create_view(&Default::default())
}
fn decode(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
fn encode(x: f32) -> f32 {
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}
fn downsample(pixels: &[u8], width: u32, height: u32, srgb: bool) -> Vec<u8> {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.0; 4];
            let mut count = 0.0;
            // Proportional bins include odd final rows/columns, including 1px edges.
            for sy in y * height / h..(y + 1) * height / h {
                for sx in x * width / w..(x + 1) * width / w {
                    let p = &pixels[((sy * width + sx) * 4) as usize..];
                    let alpha = f32::from(p[3]) / 255.0;
                    count += 1.0;
                    sum[3] += alpha;
                    for c in 0..3 {
                        let v = f32::from(p[c]) / 255.0;
                        sum[c] += if srgb { decode(v) * alpha } else { v };
                    }
                }
            }
            let i = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                let v = if srgb {
                    encode(sum[c] / sum[3].max(1e-8))
                } else {
                    sum[c] / count
                };
                out[i + c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            out[i + 3] = (sum[3] / count * 255.0).round() as u8;
        }
    }
    out
}
pub fn sampler(gpu: &Gpu, info: &TextureInfo) -> wgpu::Sampler {
    use gltf::texture::{MagFilter, MinFilter as F, WrappingMode as W};
    let address = |w| match w {
        W::ClampToEdge => wgpu::AddressMode::ClampToEdge,
        W::MirroredRepeat => wgpu::AddressMode::MirrorRepeat,
        W::Repeat => wgpu::AddressMode::Repeat,
    };
    let nearest = matches!(
        info.min,
        Some(F::Nearest | F::NearestMipmapNearest | F::NearestMipmapLinear)
    );
    let mip_nearest = matches!(
        info.min,
        Some(F::NearestMipmapNearest | F::LinearMipmapNearest)
    );
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("scene glTF sampler"),
        address_mode_u: address(info.wrap_u),
        address_mode_v: address(info.wrap_v),
        mag_filter: if info.mag == Some(MagFilter::Nearest) {
            wgpu::FilterMode::Nearest
        } else {
            wgpu::FilterMode::Linear
        },
        min_filter: if nearest {
            wgpu::FilterMode::Nearest
        } else {
            wgpu::FilterMode::Linear
        },
        mipmap_filter: if mip_nearest {
            wgpu::MipmapFilterMode::Nearest
        } else {
            wgpu::MipmapFilterMode::Linear
        },
        lod_max_clamp: if matches!(info.min, Some(F::Nearest | F::Linear)) {
            0.0
        } else {
            32.0
        },
        // Fine detail seen at a grazing angle (a planet's rings) shimmers into moiré under
        // trilinear alone. wgpu allows anisotropy only when every filter is linear.
        anisotropy_clamp: if info.mag != Some(MagFilter::Nearest)
            && matches!(info.min, None | Some(F::LinearMipmapLinear))
        {
            16
        } else {
            1
        },
        ..Default::default()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_mips_average_light_and_preserve_clear_edges() {
        let m = downsample(&[255, 255, 255, 255, 0, 0, 0, 255], 2, 1, true);
        assert!((i32::from(m[0]) - 188).abs() <= 1);
        let m = downsample(&[255, 0, 0, 255, 0, 0, 0, 0], 2, 1, true);
        assert_eq!(m, [255, 0, 0, 128]);
    }
}
