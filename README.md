# HobbyCAM

A simple 2D CAM program for hobby 3-axis CNC milling machines.
Import DXF drawings (AutoCAD, QCAD), pick contours, assign engrave / contour / pocket / drill
operations and export LinuxCNC G-code.

![main window](https://norberttak.github.io/hobbycam/images/main-window.png)

## Download

Windows (64-bit) and Apple-silicon macOS builds are on the [Releases page](https://github.com/norberttak/hobbycam/releases).
See the [install chapter](docs/src/install.md) of the manual. Privacy: [PRIVACY.md](PRIVACY.md) (no data collected).

## Build

Requires [Rust](https://rustup.rs).

```
cargo run --release -p hobbycam
```

## Documentation

The user manual is in [docs/](docs/) (mdBook: `mdbook serve docs --open`).
Online: <https://norberttak.github.io/hobbycam/> (published from the first release on).

## License

GPL-3.0-or-later, see [LICENSE](LICENSE).

## Contributing

Contributions are accepted under GPL-3.0-or-later and you agree that they may also be distributed through app stores.
Maintainers: see [RELEASING.md](RELEASING.md).

## Third-party fonts

- Phosphor Icons (regular, `@phosphor-icons/web` 2.1.2), MIT License, Copyright (c) 2020-2021 Phosphor Icons.
  See [app/assets/PHOSPHOR-LICENSE.txt](app/assets/PHOSPHOR-LICENSE.txt).
- Hershey Roman Simplex (`futural.jhf`, single-line text), from the Usenet distribution of the Hershey Fonts.
  The Hershey Fonts were originally created by Dr. A. V. Hershey while working at the U. S. National Bureau of Standards.
  The format of the Font data in this distribution was originally created by James Hurt, Cognition, Inc.
  See [crates/hc-text/fonts/HERSHEY-NOTICE.txt](crates/hc-text/fonts/HERSHEY-NOTICE.txt).
- Ubuntu Light (outline text, the copy embedded by egui via `epaint_default_fonts` 0.37), Ubuntu Font Licence 1.0.
