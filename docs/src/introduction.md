# Introduction

HobbyCAM turns 2D drawings into G-code for a hobby 3-axis milling machine running **LinuxCNC**.
You import a DXF file (or draw simple shapes yourself), pick the lines and areas you want to cut,
and tell HobbyCAM what to do with them: engrave, cut out a contour, clear a pocket or drill holes.

HobbyCAM is made for machines **without an automatic tool changer**. It writes one G-code file per tool,
so you change the bit by hand and touch off Z between files.

What HobbyCAM does **not** do:

- 3D models or 3D surfaces.
- Automatic toolpaths: you choose every operation yourself.
- Lathes, plasma cutters, lasers or 3D printers.
- Controllers other than LinuxCNC (Mach3, GRBL and so on).

The work always follows the same line:

**drawing → select → operation → check → simulate → export one file per tool**

![The HobbyCAM window with the sample bracket and three operations](images/main-window.png)

Start with [Install](install.md), then follow [Your first project](walkthrough.md).

This manual, its screenshots and the sample drawing are licensed under GPL-3.0-or-later, the same as HobbyCAM itself
(see [LICENSE](https://github.com/norberttak/hobbycam/blob/main/LICENSE)).
