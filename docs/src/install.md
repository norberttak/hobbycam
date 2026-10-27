# Install

## Download

Ready-made builds will be on the [Releases page](https://github.com/norberttak/hobbycam/releases):
a `.zip` for Windows and a `.dmg` for macOS. This section will be completed with the first release.

## Build from source

1. Install Rust with [rustup](https://rustup.rs).
2. Get the source: `git clone https://github.com/norberttak/hobbycam`
3. In the `hobbycam` folder run:

   ```text
   cargo run --release -p hobbycam
   ```

No other libraries need to be installed; everything HobbyCAM needs is built into the program.

You can also open a file directly: `hobbycam drawing.dxf` imports a DXF into a new project,
`hobbycam part.hcam` opens a saved project.

## Where your settings live

Your tool library is the file `tools.json` in your configuration folder:

| System  | Folder |
|---------|--------|
| macOS   | `~/Library/Application Support/HobbyCAM/` |
| Windows | `%APPDATA%\HobbyCAM\config\` |
| Linux   | `~/.config/hobbycam/` |

Each time the library is saved, the previous version is kept as `tools.json.bak` next to it.
Projects (`.hcam` files) are saved wherever you choose; they also keep a `.bak` of the previous save.

The menu entry **Help → User manual** opens the online copy of this manual in your web browser.
