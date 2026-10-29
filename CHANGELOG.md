# Changelog

All notable changes to HobbyCAM are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/).

## [0.3.0] - YYYY-MM-DD

### Added
- Measure tool: shows the distance, ΔX, ΔY and angle between two snapped points (temporary, not saved).

## [0.2.0] - 2026-10-28

### Added
- Settings window (File → Settings…, Cmd/Ctrl+, or the gear in the sidebar) with Units, Machine and Stock in one place.
- Move elements by dragging (with snapping); edit line, circle and rectangle sizes in Properties.
- Round corners tool: rounds clicked corners or every sharp corner of the selection with a given radius.
- The tool library shows a sketch of the tool type with its diameter, flute length, flutes and V angle.

### Changed
- New look: coloured sidebar buttons (Phosphor fill icons) grouped by colour, refreshed light and dark colours; the app
  still follows the system theme.
- The Settings menu is gone: Settings… and Tool library… are in the File menu; the sidebar has one Settings button.

### Fixed
- The Properties pane follows the last selection and is hidden when nothing is selected (it kept showing an
  operation after a click on the canvas).

## [0.1.0] - 2026-10-28

First release.

### Added
- DXF import (AutoCAD, QCAD): lines, arcs, circles, polylines, splines, ellipses, points, blocks; layers and units kept.
- Geometry core: chaining loose segments into contours, open/closed detection, orientation, islands.
- Project files (`.hcam`, with a `.bak` of the previous save) and a tool library (flat end mill, ball, V-bit, drill)
  stored in the user's settings folder; projects embed the tools they use.
- Canvas with pan/zoom, grid, layers, click and box selection.
- Operations: engrave, contour (inside/outside, climb/conventional), pocket with islands, drill (G81/G83);
  depth stepping, safe-Z rapids, strict user order.
- Holding tabs: automatic placement per contour, move/add/delete on the canvas.
- LinuxCNC G-code export: one `.ngc` file per tool for manual tool changes, separate tool-change Z.
- Sanity checks (28 rules): errors block the export, warnings ask before exporting.
- Simulation: animated tool, travelled path, swept width, play/pause/speed/scrub, machining time estimate.
- Drawing tools: line, rectangle (also rounded), circle, text (single-line Hershey and outline Ubuntu Light);
  move, rotate, scale, mirror, delete; snapping to ends, corners, centres, midpoints and intersections; set work origin;
  stock settings; undo/redo.
- User manual (Help → User manual, online at <https://norberttak.github.io/hobbycam/>).
- Release builds: Windows x64 `.zip` and Apple-silicon macOS `.dmg` (notarized), app icon, third-party licence report.
