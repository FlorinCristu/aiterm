//! Ctrl+= / Ctrl+- / Ctrl+0 zoom for the classic console window, remembered in ~/.aiterm_font.
//!
//! Windows Terminal zooms on these keys by itself; the classic console does not, and the key
//! never reaches a console program, so a low-level keyboard hook watches for it, but only acts
//! (and only swallows the key) while aiterm's own console window is in front.

pub const DEFAULT_SIZE: i16 = 16;
const MIN: i16 = 10;
const MAX: i16 = 48;

fn path() -> Option<std::path::PathBuf> {
    crate::ai::home_dir().map(|h| h.join(".aiterm_font"))
}

pub fn saved_size() -> i16 {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse().ok())
        .map(|s: i16| s.clamp(MIN, MAX))
        .unwrap_or(DEFAULT_SIZE)
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::Console::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    /// Height in pixels of the console font, keeping its face.
    pub fn set_size(px: i16) {
        unsafe {
            let out = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut font: CONSOLE_FONT_INFOEX = std::mem::zeroed();
            font.cbSize = std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32;
            if GetCurrentConsoleFontEx(out, 0, &mut font) == 0 {
                return;
            }
            font.dwFontSize = COORD { X: 0, Y: px.clamp(MIN, MAX) };
            SetCurrentConsoleFontEx(out, 0, &font);
        }
    }

    fn current_size() -> i16 {
        unsafe {
            let mut font: CONSOLE_FONT_INFOEX = std::mem::zeroed();
            font.cbSize = std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32;
            if GetCurrentConsoleFontEx(GetStdHandle(STD_OUTPUT_HANDLE), 0, &mut font) == 0 {
                return DEFAULT_SIZE;
            }
            font.dwFontSize.Y
        }
    }

    /// Change the text size but keep the window where and how big it is.
    ///
    /// The console keeps its column and row count across a font change, so the window would
    /// grow or shrink with the text; putting the old outline back makes it re-fit the text
    /// instead (fewer, bigger cells), the way a browser zoom does.
    pub fn zoom(step: i16) {
        let size = if step == 0 { DEFAULT_SIZE } else { (current_size() + step).clamp(MIN, MAX) };
        unsafe {
            let hwnd = GetConsoleWindow();
            let mut before: RECT = std::mem::zeroed();
            let mut client: RECT = std::mem::zeroed();
            let have_rect = !hwnd.is_null() && GetWindowRect(hwnd, &mut before) != 0 && GetClientRect(hwnd, &mut client) != 0;
            set_size(size);
            if have_rect {
                // A window can't be wider than its buffer: with smaller text more columns fit, so
                // widen the buffer first or the window would shrink anyway.
                let out = GetStdHandle(STD_OUTPUT_HANDLE);
                let mut font: CONSOLE_FONT_INFOEX = std::mem::zeroed();
                font.cbSize = std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32;
                let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
                if GetCurrentConsoleFontEx(out, 0, &mut font) != 0
                    && font.dwFontSize.X > 0
                    && GetConsoleScreenBufferInfo(out, &mut info) != 0
                {
                    let cols = ((client.right - client.left) / font.dwFontSize.X as i32) as i16;
                    if cols > info.dwSize.X {
                        SetConsoleScreenBufferSize(out, COORD { X: cols, Y: info.dwSize.Y });
                    }
                }
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    before.left,
                    before.top,
                    before.right - before.left,
                    before.bottom - before.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        if let Some(p) = path() {
            let _ = std::fs::write(p, size.to_string());
        }
    }

    unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 && (wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize) {
            let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
            let ctrl = GetAsyncKeyState(VK_CONTROL as i32) < 0;
            let ours = GetForegroundWindow() == GetConsoleWindow();
            if ctrl && ours {
                let step = match kb.vkCode as u16 {
                    VK_OEM_PLUS | VK_ADD => Some(2),
                    VK_OEM_MINUS | VK_SUBTRACT => Some(-2),
                    0x30 | VK_NUMPAD0 => Some(0),
                    _ => None,
                };
                if let Some(step) = step {
                    zoom(step);
                    return 1;
                }
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }

    /// Start listening for the zoom keys. Does nothing inside Windows Terminal, which has its own.
    pub fn watch_zoom_keys() {
        if std::env::var_os("WT_SESSION").is_some() || unsafe { GetConsoleWindow() }.is_null() {
            return;
        }
        std::thread::spawn(|| unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), GetModuleHandleW(std::ptr::null()), 0);
            if hook.is_null() {
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        });
    }
}

#[cfg(not(windows))]
pub fn set_size(_: i16) {}

#[cfg(not(windows))]
pub fn watch_zoom_keys() {}
