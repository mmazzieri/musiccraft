//! Files a sync service lists but has not downloaded. Reading one makes the service download
//! it, so a scan looks at the attributes first and leaves such files alone.

use std::fs::Metadata;

/// Whether reading the file would fetch it from the network.
#[cfg(windows)]
pub fn online_only(meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    // FILE_ATTRIBUTE_OFFLINE and FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, from Microsoft's public
    // "File Attribute Constants" page.
    meta.file_attributes() & (0x1000 | 0x0040_0000) != 0
}

#[cfg(target_os = "macos")]
pub fn online_only(meta: &Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    // SF_DATALESS, from the chflags(2) manual page.
    meta.st_flags() & 0x4000_0000 != 0
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn online_only(_meta: &Metadata) -> bool {
    false
}
