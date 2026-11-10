//! aiterm-app: aiterm in its own window. Each tab is aiterm.exe in a ConPTY, drawn by xterm.js
//! in a WebView2; aiterm.exe itself stays a plain console program.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod pty;
#[cfg(windows)]
mod theme;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(not(windows))]
fn main() {
    println!("aiterm-app is Windows-only; run aiterm in your terminal instead.");
}
