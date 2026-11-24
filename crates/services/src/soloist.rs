//! Spotify Soloist, Spotify's own player without a window, when the person has installed it.
//! Command line as in <https://developer.spotify.com/documentation/soloist>. It is never
//! downloaded, bundled or redistributed by MusicCraft; each person uses their own key.

use crate::ServiceError;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// The exit code of a build past its 90 days.
const EXPIRED: i32 = 10;

/// A running player. It ends with MusicCraft.
pub struct Soloist {
    child: Child,
}

fn program() -> &'static str {
    if cfg!(windows) { "soloist.exe" } else { "soloist" }
}

/// The program at the place the person chose, or on the search path.
pub fn find(chosen: &str) -> Option<PathBuf> {
    if !chosen.trim().is_empty() {
        let p = PathBuf::from(chosen.trim());
        let p = if p.is_dir() { p.join(program()) } else { p };
        return p.is_file().then_some(p);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(program())).find(|p| p.is_file())
}

impl Soloist {
    pub fn start(path: &Path, device_name: &str, key: &str) -> Result<Soloist, ServiceError> {
        let mut cmd = Command::new(path);
        cmd.arg("--device-name").arg(device_name).arg("--api-key").arg(key);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console window beside MusicCraft's.
            cmd.creation_flags(0x0800_0000);
        }
        let child = cmd.spawn().map_err(|e| ServiceError::Other(e.to_string()))?;
        Ok(Soloist { child })
    }

    /// Why it is no longer running, or nothing while it runs.
    pub fn ended(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(s)) if s.code() == Some(EXPIRED) => {
                Some("this build of Spotify Soloist has expired: download a newer one".to_owned())
            }
            Ok(Some(s)) => Some(format!("Spotify Soloist stopped ({s})")),
            Err(e) => Some(e.to_string()),
        }
    }
}

impl Drop for Soloist {
    fn drop(&mut self) {
        // Already ended, or ended here.
        self.child.kill().ok();
        self.child.wait().ok();
    }
}
