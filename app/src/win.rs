//! The few raw Win32 calls the host needs: clipboard, message box, OS build, the side-channel pipe.

use std::io::{BufRead, BufReader};
use std::os::windows::io::FromRawHandle;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND};
use windows_sys::Win32::System::DataExchange::*;
use windows_sys::Win32::System::Memory::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CF_UNICODETEXT: u32 = 13;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn message_box(text: &str) {
    unsafe { MessageBoxW(std::ptr::null_mut(), wide(text).as_ptr(), wide("aiterm").as_ptr(), MB_OK | MB_ICONERROR) };
}

/// The owner window matters: with none, SetClipboardData fails after EmptyClipboard.
pub fn set_clipboard(hwnd: isize, text: &str) {
    let w = wide(&text.replace("\r\n", "\n").replace('\n', "\r\n"));
    unsafe {
        if OpenClipboard(hwnd as HWND) == 0 {
            return;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2);
        if !h.is_null() {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                GlobalUnlock(h);
                if SetClipboardData(CF_UNICODETEXT, h).is_null() {
                    GlobalFree(h);
                }
            }
        }
        CloseClipboard();
    }
}

pub fn clipboard(hwnd: isize) -> Option<String> {
    unsafe {
        if OpenClipboard(hwnd as HWND) == 0 {
            return None;
        }
        let h = GetClipboardData(CF_UNICODETEXT);
        let p = if h.is_null() { std::ptr::null() } else { GlobalLock(h) as *const u16 };
        let text = (!p.is_null()).then(|| {
            let n = (0..).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            GlobalUnlock(h);
            s
        });
        CloseClipboard();
        text
    }
}

/// xterm.js adapts its reflow and wrapping to the ConPTY of this Windows build.
pub fn build_number() -> u32 {
    let mut buf = [0u16; 16];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").as_ptr(),
            wide("CurrentBuildNumber").as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    } == ERROR_SUCCESS;
    if !ok {
        return 0;
    }
    String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]).trim().parse().unwrap_or(0)
}

/// Accept clients on `name` forever, handing every line they write to `on_line`.
/// One instance per client; inbound only, local only, and we must own the first instance.
pub fn serve_pipe(name: &str, on_line: impl Fn(String) + Send + Clone + 'static) {
    let name = wide(name);
    let mut first = FILE_FLAG_FIRST_PIPE_INSTANCE;
    loop {
        let h = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_INBOUND | first,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                0,
                4096,
                0,
                std::ptr::null(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return;
        }
        first = 0;
        let connected = unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) } != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if !connected {
            unsafe { CloseHandle(h) };
            continue;
        }
        let file = unsafe { std::fs::File::from_raw_handle(h as _) };
        let on_line = on_line.clone();
        std::thread::spawn(move || BufReader::new(file).lines().map_while(Result::ok).for_each(on_line));
    }
}
