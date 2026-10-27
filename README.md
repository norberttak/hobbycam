# HobbyCAM

A simple 2D CAM program for hobby 3-axis CNC milling machines.
Import DXF drawings (AutoCAD, QCAD), pick contours, assign engrave / contour / pocket / drill
operations and export LinuxCNC G-code.

Status: early development – see [PLAN.md](PLAN.md).

## Build

Requires [Rust](https://rustup.rs).

```
cargo run --release -p hobbycam
```

## License

GPL-3.0-or-later, see [LICENSE](LICENSE).

## Third-party fonts

- Phosphor Icons (regular, `@phosphor-icons/web` 2.1.2), MIT License, Copyright (c) 2020-2021 Phosphor Icons.
  See [app/assets/PHOSPHOR-LICENSE.txt](app/assets/PHOSPHOR-LICENSE.txt).
- Hershey Roman Simplex (`futural.jhf`, single-line text), from the Usenet distribution of the Hershey Fonts.
  The Hershey Fonts were originally created by Dr. A. V. Hershey while working at the U. S. National Bureau of Standards.
  The format of the Font data in this distribution was originally created by James Hurt, Cognition, Inc.
  See [crates/hc-text/fonts/HERSHEY-NOTICE.txt](crates/hc-text/fonts/HERSHEY-NOTICE.txt).
- Ubuntu Light (outline text, the copy embedded by egui via `epaint_default_fonts` 0.37), Ubuntu Font Licence 1.0.
