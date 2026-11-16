//! The live frontends of a ctxloom home (owner, 2026-11-15): each
//! interactive session's frontend holds `<home>/.ctxloom/frontends/
//! <pid>.lock` for its life, so `ctxloom shutdown` can find every one, the
//! daemon's or none's (a frontend outlives its daemon), and ask it to stop.
//! The ask is a file, `<pid>.stop` beside the lock, which the frontend
//! watches: no signal and no process id is trusted, only the lock (a dead
//! frontend's reads free, and its files are taken away by the next look).

use std::path::PathBuf;
use std::sync::Arc;

use file_system::{Fs, Perms};
use os::Mode;
use session_ports::{LockError, LockState};

/// The directory under ctxloom's home the frontends' locks are in.
const DIR: &str = "frontends";

/// The live frontends' locks under ctxloom's home.
pub struct FrontendLocks {
    fs: Arc<dyn Fs>,
    home: PathBuf,
}

impl FrontendLocks {
    /// The frontends of ctxloom's home `home`, through `fs`.
    pub fn new(fs: Arc<dyn Fs>, home: impl Into<PathBuf>) -> FrontendLocks {
        FrontendLocks {
            fs,
            home: home.into(),
        }
    }

    fn dir(&self) -> PathBuf {
        self.home.join(".ctxloom").join(DIR)
    }

    /// Where the frontend `pid`'s lock is.
    pub fn lock_path(&self, pid: u32) -> PathBuf {
        self.dir().join(format!("{pid}.lock"))
    }

    /// Where a stop is asked of the frontend `pid`.
    pub fn stop_path(&self, pid: u32) -> PathBuf {
        self.dir().join(format!("{pid}.stop"))
    }

    /// Hold the frontend `pid`'s lock for its life; a stop asked of an
    /// earlier process of its id is taken away first.
    pub fn hold(&self, pid: u32) -> Result<FrontendHeld, LockError> {
        let _ = self.fs.remove_file(&self.stop_path(pid));
        let locked = crate::take(&*self.fs, &self.lock_path(pid))?;
        Ok(FrontendHeld {
            fs: Arc::clone(&self.fs),
            lock: self.lock_path(pid),
            stop: self.stop_path(pid),
            locked: Some(locked),
        })
    }

    /// The process ids of the frontends alive now, in order; a dead one's
    /// files are taken away.
    pub fn live(&self) -> Vec<u32> {
        let Ok(entries) = self.fs.read_dir(&self.dir()) else {
            return Vec::new();
        };
        let mut pids: Vec<u32> = entries
            .iter()
            .filter_map(|entry| {
                let name = entry.name.to_str()?;
                name.strip_suffix(".lock")?.parse().ok()
            })
            .filter(|pid| match crate::probe(&*self.fs, &self.lock_path(*pid)) {
                LockState::Held => true,
                LockState::Free => {
                    let _ = self.fs.remove_file(&self.lock_path(*pid));
                    let _ = self.fs.remove_file(&self.stop_path(*pid));
                    false
                }
                LockState::Unproven => false,
            })
            .collect();
        pids.sort_unstable();
        pids
    }

    /// Ask the frontend `pid` to stop: its stop file written.
    pub fn ask_to_stop(&self, pid: u32) -> std::io::Result<()> {
        self.fs.write_atomic(
            &self.stop_path(pid),
            b"stop\n",
            Perms::Exact(Mode::from_bits(0o600)),
        )
    }

    /// Whether a stop was asked of the frontend `pid`.
    pub fn stop_asked(&self, pid: u32) -> bool {
        self.fs.exists(&self.stop_path(pid)).unwrap_or(false)
    }
}

/// A frontend's lock held: its stop file and its lock file are taken away,
/// then the lock let go, when it is dropped.
pub struct FrontendHeld {
    fs: Arc<dyn Fs>,
    lock: PathBuf,
    stop: PathBuf,
    locked: Option<file_lock::Locked>,
}

impl Drop for FrontendHeld {
    fn drop(&mut self) {
        let _ = self.fs.remove_file(&self.stop);
        let _ = self.fs.remove_file(&self.lock);
        drop(self.locked.take());
    }
}

#[cfg(test)]
#[path = "frontends.test.rs"]
mod tests;
