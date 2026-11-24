//! Adds files to the library by copying them into `Music/<Album artist>/<Album>/`.

use crate::model::Info;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub copied: usize,
    /// Already in the library, byte for byte.
    pub duplicates: usize,
    pub failed: Vec<(PathBuf, String)>,
}

/// Every playable file in `paths`; folders are searched.
pub fn collect(paths: &[PathBuf]) -> Vec<PathBuf> {
    fn add(path: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(meta) = std::fs::symlink_metadata(path) else { return };
        if meta.is_dir() && depth < 24 {
            let Ok(entries) = std::fs::read_dir(path) else { return };
            let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            children.sort();
            for c in children {
                add(&c, depth + 1, out);
            }
        } else if meta.is_file() && path.file_name().and_then(|n| n.to_str()).is_some_and(crate::is_audio) {
            out.push(path.to_owned());
        }
    }
    let mut out = Vec::new();
    for p in paths {
        add(p, 0, &mut out);
    }
    out
}

/// Copies `files` into the music folder. `progress` is told after each file.
pub fn copy_in(music: &Path, files: &[PathBuf], mut progress: impl FnMut(usize)) -> Report {
    let mut report = Report::default();
    for (i, file) in files.iter().enumerate() {
        if file.starts_with(music) {
            report.duplicates += 1;
        } else {
            match place(music, file) {
                Ok((_, true)) => report.copied += 1,
                Ok((_, false)) => report.duplicates += 1,
                Err(e) => report.failed.push((file.clone(), e)),
            }
        }
        progress(i + 1);
    }
    report
}

/// Copies one file into the music folder. Gives back where it is there, and whether it is new.
pub fn place(music: &Path, file: &Path) -> Result<(PathBuf, bool), String> {
    if file.starts_with(music) {
        return Ok((file.to_owned(), false));
    }
    let name = file.file_name().and_then(|n| n.to_str()).ok_or("the file name is not valid text")?;
    let info = crate::tags::read(file, name)?;
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase();
    let dir = music.join(album_folder(&info));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = file_stem(&info);
    for n in 1..1000u32 {
        let target = dir.join(if n == 1 { format!("{base}.{ext}") } else { format!("{base} ({n}).{ext}") });
        if target.exists() {
            if same_bytes(file, &target).unwrap_or(false) {
                return Ok((target, false));
            }
            continue;
        }
        // Copied under a name the scan ignores, so a half-copied file is never listed.
        let part = dir.join(format!(".{base}.part"));
        let done = std::fs::copy(file, &part).and_then(|_| std::fs::rename(&part, &target));
        if let Err(e) = done {
            // The copy failed; nothing else uses the partial file.
            std::fs::remove_file(&part).ok();
            return Err(e.to_string());
        }
        return Ok((target, true));
    }
    Err("too many files with this name".to_owned())
}

/// The path below the music folder, as the library writes it.
pub fn below(music: &Path, file: &Path) -> Option<String> {
    use unicode_normalization::UnicodeNormalization;
    let parts: Option<Vec<String>> = file
        .strip_prefix(music)
        .ok()?
        .components()
        .map(|c| Some(c.as_os_str().to_str()?.nfc().collect::<String>()))
        .collect();
    parts.map(|p| p.join("/")).filter(|p| !p.is_empty())
}

fn same_bytes(a: &Path, b: &Path) -> std::io::Result<bool> {
    let (ma, mb) = (std::fs::metadata(a)?, std::fs::metadata(b)?);
    if ma.len() != mb.len() || crate::cloud::online_only(&mb) {
        return Ok(false);
    }
    let (mut fa, mut fb) = (std::fs::File::open(a)?, std::fs::File::open(b)?);
    let (mut ba, mut bb) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let n = fa.read(&mut ba)?;
        if n == 0 {
            return Ok(true);
        }
        let (Some(x), Some(y)) = (ba.get(..n), bb.get_mut(..n)) else { return Ok(false) };
        fb.read_exact(y)?;
        if x != &*y {
            return Ok(false);
        }
    }
}

fn album_folder(info: &Info) -> PathBuf {
    let artist = if info.album_artist.is_empty() { &info.artist } else { &info.album_artist };
    let mut p = PathBuf::from(safe(artist, "Unknown Artist"));
    p.push(safe(&info.album, "Unknown Album"));
    p
}

fn file_stem(info: &Info) -> String {
    let title = safe(&info.title, "Untitled");
    match (info.disc, info.track) {
        (_, 0) => title,
        (0 | 1, t) => format!("{t:02} {title}"),
        (d, t) => format!("{d}-{t:02} {title}"),
    }
}

/// A file or folder name every system and sync service accepts.
pub fn safe(name: &str, otherwise: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let mapped: String =
        name.nfc().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).take(80).collect();
    let s = mapped.trim().trim_matches('.').trim();
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(|c: char| c.is_ascii_digit()));
    match (s.is_empty(), reserved) {
        (true, _) => otherwise.to_owned(),
        (_, true) => format!("_{s}"),
        _ => s.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::safe;

    /// Names from tags become names Windows and the sync services accept.
    #[test]
    fn names_are_safe_everywhere() {
        assert_eq!(safe("AC/DC: Live?", "x"), "AC_DC_ Live_");
        assert_eq!(safe("..", "x"), "x");
        assert_eq!(safe("con.mp3", "x"), "_con.mp3");
        assert_eq!(safe("  ", "x"), "x");
        assert!(safe(&"é".repeat(500), "x").chars().count() <= 80);
    }
}
