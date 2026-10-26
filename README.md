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
