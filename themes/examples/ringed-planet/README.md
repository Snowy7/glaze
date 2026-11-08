# Ringed planet

Original mypaper test asset, dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
The planet, textures and generator may be copied, modified and distributed without attribution.

`planet.glb` exercises base-color, metallic/roughness, normal and emissive maps,
translucent double-sided rings, and named `Turntable` (node rotation) and
`Breathe` (morph weight) clips. No external files are needed.

Rebuild with `py generate.py` (Pillow required). Use `animation: "Turntable"`
or `animation: "Breathe"` in a model layer; `animation: null` is a still.
