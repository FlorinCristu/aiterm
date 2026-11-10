// Embeds the aiterm icon (resource 1, also used for the window) and version info into aiterm-app.exe.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../packaging/aiterm.ico")
            .set("ProductName", "aiterm")
            .set("FileDescription", "aiterm");
        res.compile().expect("failed to embed Windows resources");
    }
    println!("cargo:rerun-if-changed=../packaging/aiterm.ico");
    println!("cargo:rerun-if-changed=assets");
}
