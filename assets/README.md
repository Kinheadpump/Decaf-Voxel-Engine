Texture assets live in `assets/textures`.

Rules:
- Every registered texture name maps to a PNG file with the same name.
- Example: `BlockTextures::all("stone")` loads `assets/textures/stone.png`.
- All texture PNGs must use the same dimensions.
- The current renderer expects a simple 2D texture array, so mixing sizes is not supported.

Per-face registration examples:

```rust
BlockTextures::all("stone")
BlockTextures::top_bottom_sides("grass_top", "dirt", "grass_side")
BlockTextures::explicit("px", "nx", "py", "ny", "pz", "nz")
```

Starter placeholder textures are included for the currently registered default blocks. You can replace any PNG with your own art as long as the file name and dimensions stay consistent.
