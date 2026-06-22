use fat_vfs::{Metadata, NodeType, Permissions};
use std::fs;

/// Converts [`std::fs::Metadata`] into [`fat_vfs::Metadata`].
#[must_use]
pub fn into_vfs_metadata(metadata: fs::Metadata) -> Metadata {
    let ft = metadata.file_type();
    Metadata {
        mode: if metadata.permissions().readonly() {
            Permissions::READ
        } else {
            Permissions::READ_WRITE
        },
        size: metadata.len(),
        ty: if ft.is_file() {
            NodeType::File
        } else if ft.is_dir() {
            NodeType::Directory
        } else if ft.is_symlink() {
            NodeType::Symlink
        } else {
            NodeType::Unknown
        },
    }
}
