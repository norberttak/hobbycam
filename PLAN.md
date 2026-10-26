# HobbyCAM – Implementation Plan

## Decisions (from planning session, 2026-10-26)

| Topic | Decision |
|---|---|
| Language | Rust (stable), single self-contained binary |
| GUI | `egui` / `eframe` |
| G-code | LinuxCNC only in v1, behind a `PostProcessor` trait so other dialects can be added later |
| Units | User-selectable mm / inch. Internally everything is stored in **mm** (f64); conversion happens only at UI and G-code output |
| Operations | Engrave (on line), Contour (inside / outside), Pocket, Drill |
| Text | Single-line (stroke) font for engraving **and** outline (TTF) fonts |
| Project file | JSON (`.hcam`), with a `format_version` field |
| Tool change | Manual: `T<n> M6` (LinuxCNC manual toolchange) + `M0` pause, comment with tool name |
| License | GPL-3.0-or-later; all dependencies must be GPL-compatible (MIT/Apache/BSD/OFL) |
| Distribution | GitHub Releases (Windows .zip, macOS signed+notarized .dmg) and Mac App Store |

Defaults chosen without asking (change any time):
- Work zero: X0/Y0 = drawing origin, Z0 = **top of stock**. Cutting depths are positive numbers in the UI, emitted as negative Z.
- Arcs are emitted as native `G2/G3`; splines/ellipses are flattened to arcs/lines with a user tolerance (default 0.01 mm).
- Drilling uses `G81` (plain) or `G83` (peck) canned cycles.

## Candidate crates

| Need | Crate | License |
|---|---|---|
| GUI | `eframe`, `egui` | MIT/Apache |
| DXF read | `dxf` | MIT |
| Offsets / pocket rings (keeps arcs) | `cavalier_contours` | MIT/Apache |
| Boolean ops (if needed) | `i_overlay` | MIT |
| JSON | `serde`, `serde_json` | MIT/Apache |
| TTF outlines | `ttf-parser` | MIT/Apache |
| Single-line font | Hershey font data (public domain), embedded | PD |
| Bundled outline font | an OFL font (e.g. Liberation Sans / DejaVu) | OFL |
| File dialogs | `rfd` | MIT |
| Config dir (tool library) | `directories` | MIT/Apache |

Rule: no dependency that needs a system library at runtime.

## Architecture

Cargo workspace, logic separated from GUI so it is unit-testable without a window:

```
hobbycam/
  crates/
    hc-geom/      geometry: Line, Arc, Circle, Polyline(bulge), contours, chaining, bbox, transforms
    hc-dxf/       DXF -> hc-geom (layers, units from $INSUNITS)
    hc-text/      text -> geometry (Hershey + TTF)
    hc-project/   Project, Tool library, Operations, JSON (de)serialization
    hc-toolpath/  operations -> Toolpath (list of moves); tabs; sanity checks
    hc-post/      PostProcessor trait + LinuxCNC implementation
    hc-sim/       move-list interpreter for simulation (time, position at t)
  app/            eframe GUI (canvas, panels, dialogs) – thin layer over the crates
  docs/           user manual (mdBook)
  tests/fixtures/ DXF samples (QCAD, AutoCAD), golden G-code files
```

Core data flow:
`DXF / drawing tools -> Geometry -> (user selects + Operation params) -> Toolpath -> Sanity check -> PostProcessor -> .ngc`
`Toolpath -> Simulation`

## Milestones

Each milestone ends with: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all green on macOS and Windows CI, then user sign-off before the next one.

### M0 – Project setup
- git repo, GPL LICENSE, README, workspace skeleton, `.gitignore`
- GitHub Actions CI: fmt, clippy, test on `macos-latest` + `windows-latest` (+ ubuntu, cheap)
- **Done when:** empty eframe window opens on Mac and Windows; CI green.

### M1 – Geometry core (`hc-geom`)
- Entities: Line, Arc, Circle, Polyline with bulges; bbox; translate/rotate/scale/mirror
- Chaining loose segments into contours (with tolerance), closed/open detection, orientation (CW/CCW)
- Point-in-polygon, containment tree (which contour is inside which – needed for pockets with islands)
- **Tests:** chaining, closure, orientation, containment, transforms.

### M2 – DXF import (`hc-dxf`)
- LINE, ARC, CIRCLE, LWPOLYLINE, POLYLINE, SPLINE, ELLIPSE, POINT (drill centers); INSERT/blocks (exploded)
- Layers kept (useful for selection); units from `$INSUNITS`
- **Tests:** fixture files exported from QCAD and AutoCAD (you provide a few real ones).

### M3 – Project file & tool inventory (`hc-project`)
- Tool: id, name, type (flat end mill, ball, V-bit, drill), diameter, flute length, flutes, default feed/plunge/RPM/depth-per-pass, comment
- Tool library stored as JSON in the user config dir; project embeds a copy of used tools (project stays portable)
- Project: units, stock (size, thickness), geometry, operations, settings; `format_version`
- **Tests:** round-trip save/load, version check, unit conversion.

### M4 – GUI shell (`app`)
- Canvas with pan/zoom, grid, origin marker; layer list
- Select by click / box; selection highlight
- Tool library dialog (add/edit/delete)
- Open DXF, open/save project, unit switch
- **Tests:** logic behind the UI (hit-testing, view transforms) unit-tested; UI checked manually.

### M5 – Toolpaths (`hc-toolpath`)
- Engrave: follow the line at depth
- Contour: offset by tool radius inside/outside (cavalier_contours), climb/conventional
- Pocket: concentric offset rings with stepover, islands respected
- Drill: circle centers / points -> G81/G83
- Depth stepping (depth per pass), ramp or plunge entry, safe-Z rapids, operation ordering (by tool, then user order)
- **Tests:** offsets of known shapes, pocket ring counts, depth pass counts, entry/exit at safe Z.

### M6 – Holding tabs
- Auto-place N tabs per closed contour (evenly by length, avoiding corners/arcs ends), tab width + height
- User can move/delete/add a tab on canvas
- Toolpath lifts over tabs only on passes below tab top
- **Tests:** tab count/positions, Z profile across a tab.

### M7 – G-code output (`hc-post`)
- `PostProcessor` trait; `LinuxCnc` implementation
- Header (`G17 G21|G20 G90 G94`, program comment), tool change block (`M5`, `G0 Z<safe>`, `T<n> M6`, `M0` + comment, `S<rpm> M3`), footer (`M5 M2`)
- **Tests:** golden-file tests (expected `.ngc` for a few fixture projects).

### M8 – Sanity checks
Run before export; errors block export, warnings don't:
- Pocket/contour feature smaller than tool diameter, or offset produced nothing
- Depth greater than tool flute length; depth per pass > depth; zero/negative feed, RPM, depth
- Pocket/inside contour on an open contour
- Toolpath outside stock, or deeper than stock thickness (warn: through-cut is often intended)
- Drill tool diameter ≠ hole diameter (warn); operation references missing tool
- Tabs on a contour that is not cut through (warn)
- **Tests:** one test per rule (positive and negative case).

### M9 – Simulation (`hc-sim` + canvas)
- Interpret the toolpath move list: position at time t from feeds
- Canvas: animated tool, travelled path, swept material width (2D), play/pause/speed/scrub, Z shown as number + color
- Estimated machining time
- **Tests:** timing of known paths, position interpolation on lines and arcs.

### M10 – Drawing tools
- Line, rectangle (incl. rounded), circle, text (Hershey single-line + TTF outline), numeric input for exact sizes
- Move/rotate/scale/mirror, delete
- **Tests:** generated geometry dimensions; text produces closed contours (TTF) / open strokes (Hershey).

### M11 – User documentation
- mdBook in `docs/`: install, first project walkthrough, operations, tools, tabs, sanity messages, LinuxCNC setup notes. Screenshots from the real app.

### M12 – Release
- GitHub Actions on tag `v*`: build Windows (.zip) and macOS universal binary (.app in .dmg), sign + notarize with your Developer ID (secrets in GitHub)
- Mac App Store: app sandbox entitlements (user-selected file read/write), bundle via `cargo-bundle`, signed with the App Store certificate, upload via `xcrun altool`/Transporter, App Store Connect listing
- Walkthrough doc for both.

## Agent workflow

Three subagents live in `.claude/agents/`. The main session is the orchestrator and the only one that talks to you.

```
        you (sign-off per milestone)
                 ▲
          orchestrator (main session)
        ┌────────┼──────────┐
   architect → coder → tester
        ▲                   │
        └──── review ◄──────┘ (fails go back to coder)
```

1. **architect** – for the next milestone writes a short task spec into `specs/M<n>.md`: modules, public API signatures, acceptance tests to exist, risks. Reviews the coder's diff against the spec at the end. Does not write production code.
2. **coder** – implements the spec, with unit tests for the logic it writes. Keeps changes inside the milestone scope.
3. **tester** – independently adds edge-case tests from the spec's acceptance list, runs `fmt`/`clippy`/`test`, reports failures with exact output. Does not fix production code (sends it back to coder).
4. Loop coder ↔ tester until green, architect reviews, orchestrator shows you the result. Commit only after your OK.

## Prerequisite on your machine

Rust is not installed yet. Install it once (needed for building and testing):

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
