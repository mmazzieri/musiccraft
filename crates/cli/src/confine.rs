//! The folders an agent may touch, as the Craft apps' RFC-0015 asks: a read root and a write root,
//! compared after links are resolved, with hard links refused.
//!
//! Not everything in the RFC is here. The check and the open are two steps (the shared crate the
//! RFC foresees, with a single confined open, does not exist yet), so a folder swapped between the
//! two by another program is not caught: a root must not be writable by whoever the boundary
//! protects against. On Windows the standard library does not report link counts, so hard links
//! are refused only elsewhere.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// The read and write roots of a confined process, resolved.
#[derive(Debug, Clone)]
pub struct Confinement {
    read: Vec<PathBuf>,
    write: Vec<PathBuf>,
}

impl Confinement {
    /// Fails when there is no read root or no write root, or one is not a folder.
    pub fn new(read: &[PathBuf], write: &[PathBuf]) -> Result<Self, String> {
        if read.is_empty() || write.is_empty() {
            return Err("a read root and a write root are both needed".to_owned());
        }
        let resolve = |roots: &[PathBuf]| {
            roots
                .iter()
                .map(|root| match root.canonicalize() {
                    Ok(path) if path.is_dir() => Ok(path),
                    Ok(_) => Err(format!("the root {} is not a folder", root.display())),
                    Err(e) => Err(format!("the root {}: {e}", root.display())),
                })
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(Self { read: resolve(read)?, write: resolve(write)? })
    }

    /// `path` resolved, or a refusal that names it. A relative path starts from the first write root.
    pub fn check(&self, path: &Path, access: Access) -> Result<PathBuf, String> {
        let full = match self.write.first() {
            Some(root) => root.join(path),
            None => path.to_owned(),
        };
        let resolved = resolve(&full).ok_or_else(|| self.refusal(path, access))?;
        let inside = |roots: &[PathBuf]| roots.iter().any(|root| resolved.starts_with(root));
        if !(inside(&self.write) || access == Access::Read && inside(&self.read)) {
            return Err(self.refusal(path, access));
        }
        if has_other_names(&resolved) {
            return Err(format!("{}: the file has another name (a hard link)", path.display()));
        }
        Ok(resolved)
    }

    fn refusal(&self, path: &Path, access: Access) -> String {
        let what = match access {
            Access::Read => "reading",
            Access::Write => "writing",
        };
        let list = |roots: &[PathBuf]| roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", ");
        format!(
            "{}: {what} is refused outside the roots (read: {}; write: {})",
            path.display(),
            list(&self.read),
            list(&self.write)
        )
    }
}

/// Follows every link of `path`. What does not exist yet hangs from its nearest existing folder;
/// a `..` there is not resolved but refused, and so is a link that leads nowhere.
fn resolve(path: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        match existing.canonicalize() {
            Ok(mut resolved) => {
                resolved.extend(missing.iter().rev());
                return Some(resolved);
            }
            Err(_) if existing.symlink_metadata().is_ok() => return None,
            Err(_) => match existing.components().next_back()? {
                Component::Normal(name) => {
                    missing.push(name);
                    existing = existing.parent()?;
                }
                _ => return None,
            },
        }
    }
}

#[cfg(unix)]
pub(crate) fn has_other_names(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.nlink() > 1)
}

#[cfg(not(unix))]
pub(crate) fn has_other_names(_path: &Path) -> bool {
    false
}

/// The roots of an `mcp` command line. Any other argument is an error.
pub fn roots(args: &[String]) -> Result<Confinement, String> {
    let (mut read, mut write) = (Vec::new(), Vec::new());
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let list = match flag.as_str() {
            "--automation-read-root" => &mut read,
            "--automation-write-root" => &mut write,
            other => return Err(format!("{other} is not an option of mcp")),
        };
        list.push(PathBuf::from(args.next().ok_or_else(|| format!("{flag} needs a folder"))?));
    }
    Confinement::new(&read, &write)
}

/// Refuses a folder that holds links: whatever works inside it would follow them.
pub fn no_links(dir: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let meta = path.symlink_metadata().map_err(|e| format!("{}: {e}", path.display()))?;
        if meta.is_symlink() || has_other_names(&path) {
            return Err(format!("{}: links inside the library are refused", path.display()));
        }
        if meta.is_dir() {
            no_links(&path)?;
        }
    }
    Ok(())
}
