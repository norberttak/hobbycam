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
| Tool change | Manual, between files: one `.ngc` per tool segment, tool named in the header comment, ends at `tool_change_z` (no `T`/`M6`/`M0`) |
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
- Depth stepping (equal steps, each <= depth per pass), straight plunge entry (ramp later), safe-Z rapids, operations in strict user order (M8 may warn about repeated tool changes)
- App: automatic regeneration, toolpath display (show/hide), per-op errors; auto-fit stock to the drawing on first DXF import
- **Tests:** offsets of known shapes, pocket ring counts, depth pass counts, entry/exit at safe Z.

### M6 – Holding tabs
- Auto-place N tabs per closed contour (evenly by length, avoiding corners/arcs ends), tab width + height
- User can move/delete/add a tab on canvas
- Toolpath lifts over tabs only on passes below tab top
- **Tests:** tab count/positions, Z profile across a tab.

### M7 – G-code output (`hc-post`)
- `PostProcessor` trait; `LinuxCnc` implementation
- One file per tool segment (maximal run of consecutive enabled ops with the same tool, user order kept): `<stem>_<k>_T<n>.ngc`, a single segment gives `<stem>.ngc`. The user changes the bit and re-zeroes Z between files.
- Header (program, tool and "load T<n>, set Z0" comments; `G17 G21|G20 G40 G49 G80 G90 G91.1 G94 G98`), per op `S<rpm> M3` + dwell, footer (`G0 Z<tool_change_z>`, `M5`, `M2`); no `T`/`M6`/`M0`/`G43`
- Machine setting `tool_change_z` (≥ safe Z, default 40 mm), project format v3
- **Tests:** golden-file tests (expected `.ngc` for a few fixture projects).

### M8 – Sanity checks
Spec: `specs/M8.md` (`hc_toolpath::check`, one Issue list, live after every regeneration and again at export). Errors block export, warnings ask "Export anyway?":
- Pocket/contour feature smaller than tool diameter, or offset produced nothing
- Depth greater than tool flute length; depth per pass > depth; zero/negative feed, RPM, depth
- Pocket/inside contour on an open contour
- Toolpath outside stock, or deeper than stock thickness (warn: through-cut is often intended)
- Drill tool diameter ≠ hole diameter (warn); operation references missing tool
- Tabs on a contour that is not cut through (warn); other tab warnings from `ToolpathNote`s and the list in specs/M6.md
- **Tests:** one test per rule (positive and negative case).

### M9 – Simulation (`hc-sim` + canvas)
- Interpret the toolpath move list: position at time t from feeds
- Canvas: animated tool, travelled path, swept material width (2D), play/pause/speed/scrub, Z shown as number + color
- Estimated machining time
- **Tests:** timing of known paths, position interpolation on lines and arcs.

### M10 – Drawing tools and work origin
Spec: `specs/M10.md`. Two parts, M10a first, each signed off on its own:
- **M10a:** one snap engine (segment ends, contour corners, arc/circle centres, drill points; 10 px) used by every canvas tool.
  **Set origin** tool: hover snaps with a marker + kind label, click sets the work origin, Esc cancels; stored as
  `WorkOrigin::Point` (project format v4; v1–v3 files still load). Settings → Stock (size, thickness, origin presets or a picked point).
  Line, rectangle (incl. rounded), circle by snapped clicks or numeric form; move/rotate/scale/mirror (numeric), delete.
  Undo/redo of every project edit (whole-project snapshots, depth 100, one step per drag/field edit; Edit menu, Cmd/Ctrl+Z,
  Cmd/Ctrl+Shift+Z, Ctrl+Y).
  Test for an island sharing whole edges with its boundary (M5 ruling e).
- **M10a.2:** snap to segment midpoints (lines, arcs) and intersections (line/arc, across and within items; bbox sweep, cached,
  capped at 5000 segments / 20 000 points); one marker shape per snap kind. Left icon sidebar (Phosphor icon font, MIT, bundled)
  with the menu actions plus delete/rotate/flip; tooltips with shortcuts, selected/disabled states, light and dark.
- **M10b:** `hc-text`: Hershey Roman Simplex single-line (open strokes) and TTF outline (Ubuntu Light, already bundled by egui,
  `ttf-parser`); Text tool.
- **Tests:** generated geometry dimensions; snap candidates and priority; origin shifts G-code only (checks/sim unchanged); format v4
  migrate; undo steps, coalescing, depth and dirty state; text produces closed contours (TTF) / open strokes (Hershey).

### M11 – User documentation
Spec: `specs/M11.md`.
- mdBook in `docs/` (14 short pages): install, interface, first project walkthrough (sample `bracket.dxf`), drawing, stock & origin,
  tools, operations, tabs, simulation, sanity message reference (all 28 rules with thresholds), G-code format + LinuxCNC setup
  (touch-off per file, air cut), shortcuts, known limitations. Docs licence GPL-3.0-or-later.
- Screenshots: light theme, offscreen kittest (`shot_docs`, ignored test) into `docs/src/images/`, committed.
- Accuracy guard test (`app/src/docs_check.rs`): every RuleId's message and severity in `sanity.md`, sidebar names/keys in
  `shortcuts.md`, all links/images/includes exist. Help → User manual opens the Pages URL.
- mdBook is a dev tool only (`cargo install mdbook`); building and publishing the book in CI is M12.

### M12 – Release
Spec: `specs/M12.md`.
- CI (push/PR): fmt, clippy, test on macOS + Windows; actionlint + shellcheck; `cargo-about` licence gate; `mdbook build docs` and
  deploy to GitHub Pages on `main`. Actions pinned by SHA, toolchain pinned in `rust-toolchain.toml`.
- Release on tag `v*` (+ manual dry run): Windows x86_64 `.zip` (static CRT, icon), macOS Apple-silicon (aarch64, no Intel) `.app` in `.dmg` built by plain
  scripts (no cargo-bundle), signed with Developer ID, notarized with `notarytool` + App Store Connect API key, stapled; unsigned
  `-unsigned.dmg` when secrets are absent. `SHA256SUMS`, third-party licence report, **draft** GitHub Release from `CHANGELOG.md`;
  tag must equal the workspace version.
- Original app icon (tiny-skia generator, committed PNG/ICO/ICNS).
- Mac App Store: local script, cargo feature `appstore` (sandbox: export picks a folder; saves without `.bak`), Apple Distribution +
  Installer certs, provisioning profile, `productbuild` `.pkg`, upload with Transporter; listing steps; GPL/App Store note.
- `RELEASING.md` maintainer walkthrough (repo creation and push are done by you); Download section of `docs/src/install.md`; `PRIVACY.md`.

## Version 0.2.0

Each milestone is on its own git branch (created from `main`) and becomes its own PR; the user reviews and merges it.

Model note: coder and tester run on Sonnet, architect on Opus. Specs must be precise and explicit (exact files, functions,
state fields, acceptance tests with concrete inputs and outputs) so the coder never has to guess.
Docs: every milestone updates the user manual in `docs/src` where behaviour changes; `app/src/docs_check.rs` keeps the docs in
sync (sanity rules, sidebar names/keys in `shortcuts.md`, links/images), so new sidebar buttons or shortcuts need a docs line.

### M13 – Properties pane follows the last selection (bug fix)
Spec: `specs/M13.md`. Branch `fix/properties-pane`.
- Bug: after selecting an operation, Properties keeps showing it even when a canvas element is then selected.
- Properties shows what was selected last (operation or canvas element); the pane is hidden when nothing is selected
  (and no drawing tool is active).
- Docs: `interface.md`, `operations.md` (Use current selection), `shortcuts.md` (`Esc`).

### M14 – Settings window
Spec: `specs/M14.md`. Branch `feature/settings-window`.
- Units, Machine and Stock (today menu items, `app/src/panels.rs` ~line 168) move into one Settings window with sections,
  opened from the sidebar and from the menu; room for later app preferences.
- Docs: `interface.md`, `stock-origin.md`, `shortcuts.md` (new sidebar button), screenshots if they show the old menu.

### M15 – Select, move and edit elements
Spec: `specs/M15.md`. Branch `feature/edit-elements`.
- Select and move canvas elements (drawn and imported DXF) by dragging, with snapping; one undo step per drag.
- Properties derived from the geometry (no project format bump if possible): full circle → centre + radius; single line →
  end points + length; axis-aligned rectangle → position + width + height (+ corner radius); anything else → position /
  bounding box, movable. Edits are undoable.
- Operations attached to an element follow it (item ids stay the same).
- Docs: `drawing.md`, `interface.md`.

### M16 – Round corners (fillet)
Spec: `specs/M16.md`. Branch `feature/round-corners`.
- Drawing tool with a radius parameter: click a single sharp corner, or apply to the selected contours to round every sharp corner.
- Corners where the radius does not fit are skipped and reported (message with the count). Drawn and DXF geometry; undoable.
- Docs: `drawing.md`, `shortcuts.md` (sidebar button / key).

### M17 – Tool schematics
Branch `feature/tool-schematics`.
- Tool inventory dialog shows a schematic of the selected kind (flat end mill, ball end mill, V-bit, drill), annotated with the
  form's parameters (diameter, flute length, flutes, V angle, …); drawn with the egui painter, no images.
- Docs: `tools.md` (+ screenshot).

### M18 – UI refresh
Branch `feature/ui-refresh`.
- Step 1, before any code: a mock-up page comparing 2–3 options (e.g. Phosphor duotone icons tinted by group with a new colour
  scheme; a full-colour set such as Fluent UI colour icons (MIT); one more), each with light and dark palettes, licence notes
  and the impact on self-containment. The user picks one.
- Step 2: implement the chosen option.
- Docs: screenshots in `docs/src/images/` regenerated (`shot_docs`), `interface.md` if names change.

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
