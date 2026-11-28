//! Whether the system can show a window for choosing files, and what to do with a path typed
//! instead. On Linux and the BSDs the chooser is the desktop portal's, or zenity's when there is
//! no portal; where neither is installed (WSL, a bare window manager) the chooser gives back
//! nothing at once, as if it had been cancelled, so the window asks for the path itself.

use std::path::PathBuf;

/// What a path is chosen for.
#[derive(Debug, Clone, PartialEq)]
pub enum Want {
    /// Files or folders to add to the library.
    Add,
    /// A folder to add to the library.
    AddFolder,
    /// The folder of the library.
    Library,
    /// A playlist file to import.
    Playlist,
    /// A library exported in XML.
    LibraryFile,
    /// A list of songs to import.
    SongList,
    /// Where a playlist is saved: its id and the file name offered.
    SavePlaylist { id: String, name: String },
}

/// The path the person typed: quotes around it and spaces at its ends are taken away, and a
/// leading `~` is the home folder. Nothing typed is no path.
pub fn typed(text: &str, home: Option<PathBuf>) -> Option<PathBuf> {
    let text = text.trim();
    let text =
        ['"', '\''].iter().find_map(|q| text.strip_prefix(*q).and_then(|t| t.strip_suffix(*q))).unwrap_or(text).trim();
    if text.is_empty() {
        return None;
    }
    let rest = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")).or((text == "~").then_some(""));
    match (rest, home) {
        (Some(rest), Some(home)) => Some(if rest.is_empty() { home } else { home.join(rest) }),
        _ => Some(PathBuf::from(text)),
    }
}

/// The home folder, for `~`.
pub fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Whether the system's file chooser can be shown.
#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
))]
pub fn available() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    // Once found, a chooser stays; one missing is looked for again, in case it was installed.
    static FOUND: AtomicBool = AtomicBool::new(false);
    if FOUND.load(Ordering::Relaxed) {
        return true;
    }
    let found = zenity() || portal();
    FOUND.store(found, Ordering::Relaxed);
    found
}

/// Elsewhere the system always has one.
#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
)))]
pub fn available() -> bool {
    true
}

/// zenity is on the search path.
#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn zenity() -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("zenity").is_file()))
}

/// The desktop portal answers for its file chooser: a portal without a backend that draws the
/// chooser has no such interface.
#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn portal() -> bool {
    use std::time::Duration;
    let Ok(builder) = zbus::blocking::connection::Builder::session() else { return false };
    // Starting the portal can take a moment; a bus that does not answer is no chooser.
    let Ok(bus) = builder.method_timeout(Duration::from_secs(5)).build() else { return false };
    bus.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.freedesktop.portal.FileChooser", "version"),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::typed;
    use std::path::PathBuf;

    /// A path pasted from a file manager or a terminal comes in quotes and with spaces; `~` is
    /// the home folder, and a name that only starts with it is left as it is.
    #[test]
    fn a_typed_path_is_read_as_the_person_means_it() {
        let home = || Some(PathBuf::from("/home/someone"));
        assert_eq!(typed("  \"/music/Rock\"  ", home()), Some(PathBuf::from("/music/Rock")));
        assert_eq!(typed("'/music/a b'", home()), Some(PathBuf::from("/music/a b")));
        assert_eq!(typed("~/Music", home()), Some(PathBuf::from("/home/someone/Music")));
        assert_eq!(typed("~", home()), home());
        assert_eq!(typed("~other/Music", home()), Some(PathBuf::from("~other/Music")));
        assert_eq!(typed("~/Music", None), Some(PathBuf::from("~/Music")));
        assert_eq!(typed(" \"\" ", home()), None);
        assert_eq!(typed("   ", home()), None);
    }
}
