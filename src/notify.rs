//! A nudge when a slow command finishes while you are looking at something else.
//!
//! The classic console flashes its taskbar button, and only when it isn't the window in front.
//! Anywhere else (Windows Terminal, other terminals) aiterm can't see the focus, so it rings the
//! bell and lets the terminal decide what that means (most flash the tab or the taskbar).

use std::time::Duration;

const SLOW: Duration = Duration::from_secs(10);

pub fn done(took: Duration) {
    if took >= SLOW && !flash() {
        print!("\x07");
    }
}

/// True when the console window is ours to judge: flashed if in the background, left alone if in front.
#[cfg(windows)]
fn flash() -> bool {
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let hwnd = GetConsoleWindow();
        // Under Windows Terminal this is a hidden stand-in window that never has the focus.
        if hwnd.is_null() || IsWindowVisible(hwnd) == 0 {
            return false;
        }
        if GetForegroundWindow() != hwnd {
            let info = FLASHWINFO {
                cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                hwnd,
                dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
                uCount: 3,
                dwTimeout: 0,
            };
            FlashWindowEx(&info);
        }
        true
    }
}

#[cfg(not(windows))]
fn flash() -> bool {
    false
}
