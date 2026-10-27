# Operations

Select geometry, click **New operation**, choose the **Kind** and the **Tool**, then **Create**.
The new operation is selected and its settings show on the right.

## Common fields

| Field | Meaning |
|---|---|
| Name | shown in the list, the Issues panel and the G-code comments ("Contour 1" by default) |
| Enabled | disabled operations are not checked, simulated or exported |
| Tool | any tool of the project or the library |
| Depth | final depth below the stock top, a positive number |
| Depth/pass | the most one pass may cut |
| Feed | cutting feed |
| Plunge feed | feed straight down |
| RPM | spindle speed |

**Use current selection** replaces the operation's geometry with what is selected now.
**Apply tool defaults** copies Depth/pass, Feed, Plunge feed, RPM (and Stepover) from the tool again.

A new operation starts with Depth equal to the tool's Depth/pass, so always set the real depth.

## How it cuts

- The depth is split into equal passes, each no deeper than Depth/pass. Depth 10 with Depth/pass 3 gives
  4 passes of 2.5 mm.
- The tool goes straight down at the Plunge feed. There is no ramp or helix, so use a tool that can plunge.
- Operations run in list order (**Up** / **Down**). Operations in a row with the same tool go into one G-code file;
  see [G-code and LinuxCNC](linuxcnc.md).

## Kinds

**Engrave** – the tool centre runs exactly on the line. Works on open and closed contours.

**Contour** – cuts along a contour.
- **Side**: **Outside** (the tool stays outside the line, for cutting out a part), **Inside** (for holes),
  **On** (on the line). Inside and Outside need closed contours; On also takes open ones.
- **Direction**: **Climb** or **Conventional**.
- If you select a part outline together with holes inside it, Outside cuts the holes from the inside.
- Contours can have [holding tabs](tabs.md).

**Pocket** – clears the whole area inside closed contours.
- Closed contours of the same operation that lie inside the boundary are **islands** and stay uncut.
- **Stepover**: distance between rings, in % of the tool diameter.
- Rings are cut from the inside out; the wall ring comes last.

**Drill** – one hole at the centre of each circle, and at each DXF point.
- Without **Peck** it is a single plunge (`G81`). Tick **Peck** and enter a peck depth to get `G83`.
- A drill uses only Depth and Plunge feed.

## What each kind accepts

| Kind | Takes | Gives an error for |
|---|---|---|
| Engrave | open and closed contours | DXF points |
| Contour Inside / Outside | closed contours | open contours ("Needs closed contours, but got …"), points |
| Contour On | open and closed contours | points |
| Pocket | closed contours | open contours ("Needs closed contours, but got …"), points |
| Drill | circles and DXF points | anything else ("Cannot use … in this operation") |

A point in an Engrave, Contour or Pocket also gives "Cannot use … in this operation".
See [Sanity checks](sanity.md) for all messages.
