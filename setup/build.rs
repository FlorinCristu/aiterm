// Embeds the payload (the folder named by AITERM_PAYLOAD, normally dist\aiterm-setup) as an
// include_bytes! table, plus the icon, version info and an asInvoker manifest.
use std::path::Path;
use std::{env, fs};

fn main() {
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("payload.rs");
    println!("cargo:rerun-if-env-changed=AITERM_PAYLOAD");
    let windows = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");

    let mut table = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    match env::var_os("AITERM_PAYLOAD").filter(|_| windows) {
        // Not an error: `cargo build/test/clippy --workspace` must work without a payload.
        None if windows => println!("cargo:warning=AITERM_PAYLOAD is not set: building aiterm-setup without a payload (use packaging\\build.ps1)"),
        None => {}
        Some(dir) => {
            let dir = fs::canonicalize(&dir).unwrap_or_else(|e| panic!("AITERM_PAYLOAD={}: {e}", Path::new(&dir).display()));
            println!("cargo:rerun-if-changed={}", dir.display());
            let mut files: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
            files.sort();
            for f in &files {
                assert!(f.is_file(), "AITERM_PAYLOAD must hold only files, found {}", f.display());
                println!("cargo:rerun-if-changed={}", f.display());
                let name = f.file_name().unwrap().to_str().expect("payload file names must be UTF-8");
                // {:?} gives valid Rust string literals, backslashes included.
                table += &format!("    ({name:?}, include_bytes!({:?})),\n", f.to_str().unwrap());
            }
            assert!(files.iter().any(|f| f.ends_with("install.ps1")), "AITERM_PAYLOAD has no install.ps1");
        }
    }
    table += "];\n";
    fs::write(&out, table).unwrap();

    if windows {
        let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("setup.manifest");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../packaging/aiterm.ico")
            .set("ProductName", "aiterm")
            .set("FileDescription", "aiterm setup")
            .set("OriginalFilename", "aiterm-setup.exe")
            // Without a requestedExecutionLevel, Windows sees "setup" in the name and asks for admin.
            .set_manifest_file(manifest.to_str().unwrap());
        res.compile().expect("failed to embed Windows resources");
    }
    println!("cargo:rerun-if-changed=../packaging/aiterm.ico");
    println!("cargo:rerun-if-changed=setup.manifest");
}
