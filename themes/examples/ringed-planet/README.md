# Ringed planet

Original mypaper test asset, dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
The planet, textures and generator may be copied, modified and distributed without attribution.

`planet.glb` exercises base-color, metallic/roughness, normal and emissive maps,
translucent double-sided rings, and named `Turntable` (node rotation) and
`Breathe` (morph weight) clips. No external files are needed.

The rings use a 4 x 1024 radial texture with broad, irregular opacity bands and soft edges.
The engine generates the complete alpha-weighted mip chain at load time; this asset requests
linear mipmap filtering. The former texture had thirty equally spaced high-contrast bands,
which produced visible stripes even with mipmaps and 16x anisotropy. Increasing filtering alone
cannot remove stripes drawn into the source. The ring mesh has 256 angular segments.

Rebuild with `py generate.py` (Pillow required). Use `animation: "Turntable"`
or `animation: "Breathe"` in a model layer; `animation: null` is a still.
