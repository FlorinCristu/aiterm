// Embeds the app icon and version info into aiterm.exe (Explorer, Alt-Tab, taskbar).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/aiterm.ico")
            .set("ProductName", "aiterm")
            .set("FileDescription", "aiterm - AI terminal");
        res.compile().expect("failed to embed Windows resources");
    }
    println!("cargo:rerun-if-changed=packaging/aiterm.ico");
}
