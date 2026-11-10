//! One tab: aiterm.exe in a ConPTY, its output buffered for the page, its input fed by a thread.

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex};

/// A reader stops reading (so the child blocks on write) once this much is waiting for the page.
const BACKLOG: usize = 4 << 20;

/// Output waiting for the page, shared with the reader thread.
#[derive(Default)]
pub struct Out {
    pub buf: Mutex<Vec<u8>>,
    drained: Condvar,
    /// Set once the tab is gone, so a reader blocked on a full buffer lets go.
    dead: AtomicBool,
}

impl Out {
    /// Up to `max` bytes for the page, waking the reader if it was waiting for room.
    pub fn take(&self, max: usize) -> Vec<u8> {
        let mut b = self.buf.lock().unwrap();
        let n = b.len().min(max);
        let chunk = b.drain(..n).collect();
        self.drained.notify_all();
        chunk
    }
}

pub struct Tab {
    pub out: Arc<Out>,
    pub cwd: PathBuf,
    /// A chunk was sent to the page and not yet acknowledged: one at a time is the flow control.
    pub in_flight: bool,
    input: Option<Sender<Vec<u8>>>,
    master: Option<Box<dyn MasterPty + Send>>,
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
}

impl Tab {
    /// A tab with no child, only text (when aiterm.exe can't be started).
    pub fn empty(cwd: PathBuf, text: &str) -> Tab {
        let out = Arc::new(Out::default());
        out.buf.lock().unwrap().extend_from_slice(text.as_bytes());
        Tab { out, cwd, in_flight: false, input: None, master: None, killer: None }
    }

    /// Start `exe` in `cwd`. `on_output` runs on the reader thread after new bytes are buffered,
    /// `on_exit` on a waiter thread when the child is gone.
    pub fn spawn(
        exe: &Path,
        cwd: PathBuf,
        size: (u16, u16),
        env: &[(&str, String)],
        on_output: impl Fn() + Send + 'static,
        on_exit: impl FnOnce() + Send + 'static,
    ) -> Result<Tab, String> {
        let pair = native_pty_system()
            .openpty(PtySize { cols: size.0.max(2), rows: size.1.max(2), pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(exe);
        cmd.cwd(&cwd);
        // Started from Windows Terminal, the host inherits its variables; the child must not
        // think it lives in Windows Terminal.
        for k in ["WT_SESSION", "WT_PROFILE_ID", "TERM_PROGRAM"] {
            cmd.env_remove(k);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let out = Arc::new(Out::default());
        let o = out.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 16 * 1024];
            while let Ok(n @ 1..) = reader.read(&mut chunk) {
                let mut b = o.buf.lock().unwrap();
                while b.len() > BACKLOG && !o.dead.load(Ordering::Relaxed) {
                    b = o.drained.wait(b).unwrap();
                }
                if o.dead.load(Ordering::Relaxed) {
                    continue; // keep draining so ClosePseudoConsole never waits on us
                }
                b.extend_from_slice(&chunk[..n]);
                drop(b);
                on_output();
            }
        });
        // Writing to the PTY blocks when the child isn't reading (a big paste); never on the UI thread.
        let (tx, rx) = channel::<Vec<u8>>();
        std::thread::spawn(move || {
            for data in rx {
                if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
                    break;
                }
            }
        });
        // ConPTY keeps the output pipe open after the child exits, so the exit comes from here.
        std::thread::spawn(move || {
            let _ = child.wait();
            on_exit();
        });
        Ok(Tab { out, cwd, in_flight: false, input: Some(tx), master: Some(pair.master), killer: Some(killer) })
    }

    pub fn write(&self, data: &[u8]) {
        if let Some(tx) = &self.input {
            let _ = tx.send(data.to_vec());
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(m) = &self.master {
            let _ = m.resize(PtySize { cols: cols.max(2), rows: rows.max(2), pixel_width: 0, pixel_height: 0 });
        }
    }

    pub fn has_child(&self) -> bool {
        self.killer.is_some()
    }

    pub fn kill(&mut self) {
        if let Some(k) = &mut self.killer {
            let _ = k.kill();
        }
    }

    /// Kill the child and close the pseudoconsole right here, for when the process is about to exit.
    pub fn close(mut self) {
        self.kill();
        self.out.dead.store(true, Ordering::Relaxed);
        self.out.drained.notify_all();
        drop(self.master.take());
    }
}

impl Drop for Tab {
    /// Closing the pseudoconsole ends everything still attached to it (bash and what it runs);
    /// off the UI thread, since older ConPTY blocks in ClosePseudoConsole.
    fn drop(&mut self) {
        self.out.dead.store(true, Ordering::Relaxed);
        self.out.drained.notify_all();
        self.input = None;
        if let Some(m) = self.master.take() {
            std::thread::spawn(move || drop(m));
        }
    }
}
