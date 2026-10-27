# Limitations

HobbyCAM is kept simple on purpose. Things to know:

- **Time estimate** is typically 10–30 % low: no acceleration, rapids assumed at 2500 mm/min, tool changes and
  touch-off not counted. The 2 s spin-up per speed change is counted.
- **Cutting**: straight plunges only (no ramp or helix), no lead-in or lead-out, no rest machining.
  Ball end mills and V-bits are treated as flat cylinders.
- **Pockets**: uncut areas smaller than r² (r = tool radius) are not reported. An island closer to the wall than the
  tool radius is kept 0.0005 mm away from it. Islands that share an edge with the boundary give the
  [SharedEdge warning](sanity.md#the-item-touches-the-item-the-pocket-treats-them-as-one-wall).
- **Tabs** are rectangular lifts, on Contour operations only.
- **DXF**: ASCII DXF from R12 to 2018. No HATCH, dimensions, text entities or binary DXF. Layers that are switched
  off in the CAD program are imported anyway. Tested with AutoCAD 2018 files only; **QCAD files are untested** so far,
  please report problems.
- No machine travel limits, no clamp collision check, no multi-line text. Drawn shapes cannot be edited after you
  create them.
- **LinuxCNC only**; no other controllers.
