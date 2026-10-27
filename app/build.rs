fn main() {
    println!("cargo:rerun-if-changed=assets/icon/hobbycam.ico");
    // exe icon + version resource; needs rc.exe from the Windows SDK
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/icon/hobbycam.ico")
        .set("ProductName", "HobbyCAM")
        .set("FileDescription", "HobbyCAM")
        .set("LegalCopyright", "GPL-3.0-or-later")
        .compile()
        .expect("Windows resource");
}
