# Install

## Download

Ready-made builds are on the [Releases page](https://github.com/norberttak/hobbycam/releases).

**Windows 10/11 (64-bit)**

1. Download `HobbyCAM-<version>-windows-x86_64.zip`.
2. Right-click the file → **Extract All**.
3. Open the extracted folder and run `hobbycam.exe`.

The program is not code-signed, so the first start shows "Windows protected your PC". Click **More info**, then
**Run anyway** (needed only once). Nothing is installed and no Visual C++ runtime is needed; to uninstall, delete the
folder. It also runs on Windows on ARM PCs (through Windows' built-in x64 emulation).

**Apple silicon Macs (M1 or later), macOS 11 or later**

1. Download `HobbyCAM-<version>-macos-arm64.dmg` and open it.
2. Drag **HobbyCAM** onto **Applications**.

The app is notarized by Apple, so it opens normally. Intel Macs are not supported.
A build named `…-unsigned.dmg` (for example from a fork of the project) is blocked on the first start: open
**System Settings → Privacy & Security** and click **Open Anyway**.

**Mac App Store** (when published): the same program with two differences required by the App Store sandbox:

- **Export G-code** asks for a **folder**; the files are named after the project.
- No `.bak` copies are kept next to projects and G-code files.

**Check a download (optional):** the Releases page also has `SHA256SUMS`. In the download folder run
`shasum -a 256 -c --ignore-missing SHA256SUMS` (macOS) or
`Get-FileHash HobbyCAM-<version>-windows-x86_64.zip` in PowerShell and compare the value with the file.

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
| macOS (App Store) | `~/Library/Containers/io.github.norberttak.hobbycam/Data/Library/Application Support/HobbyCAM/` |
| Windows | `%APPDATA%\HobbyCAM\config\` |
| Linux   | `~/.config/hobbycam/` |

Each time the library is saved, the previous version is kept as `tools.json.bak` next to it.
Projects (`.hcam` files) are saved wherever you choose; they also keep a `.bak` of the previous save
(not in the App Store version). The App Store version keeps its own tool library; it does not share it with the
`.dmg` version.

The menu entry **Help → User manual** opens the online copy of this manual in your web browser.
