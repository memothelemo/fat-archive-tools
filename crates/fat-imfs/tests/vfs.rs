//! Tests for all implemented functions in [`fat_vfs::FileSystem`]

use fat_imfs::InMemoryFs;
use fat_vfs::{FileSystem, NodeType, Permissions, VfsSnapshotNode};
use pretty_assertions::assert_eq;
use serde_json::{from_value, json};
use std::io;

#[macro_use]
#[path = "common/mod.rs"]
mod common;

#[test]
fn test_create_dir() {
    let fs = InMemoryFs::new();

    // It should create a new directory
    fs.create_dir("/foo".into()).unwrap();

    let expected = VfsSnapshotNode::directory()
        .add("foo", VfsSnapshotNode::empty_dir())
        .build();

    let result = fs.generate_snapshot().unwrap();
    assert_eq!(expected, result);

    // It should throw an error if user lacks permissions
    fs.set_permissions("/".into(), Permissions::READ).unwrap();
    assert_io_err!(fs.create_dir("/example".into()), PermissionDenied);

    // It should throw an error if a parent of the given path
    // does not exist.
    fs.set_permissions("/".into(), Permissions::READ_WRITE)
        .unwrap();

    assert_io_err!(fs.create_dir("/nested/example".into()), NotFound);

    // Path already exists
    assert_io_err!(fs.create_dir("/foo".into()), AlreadyExists);
}

#[test]
fn test_create_dir_all() {
    let fs = InMemoryFs::new();

    // It should create a new directory (same with create_dir)
    fs.create_dir("/foo".into()).unwrap();

    let expected = VfsSnapshotNode::directory()
        .add("foo", VfsSnapshotNode::empty_dir())
        .build();

    let result = fs.generate_snapshot().unwrap();
    assert_eq!(expected, result);

    // It should recursively create a directory and its missing parent components
    let fs = InMemoryFs::new();
    fs.create_dir_all("/part1/part2/part3".into()).unwrap();

    let snapshot = r#"{"part1":{"part2":{"part3":{}}}}"#;
    let expected: VfsSnapshotNode = serde_json::from_str(snapshot).unwrap();
    let result = fs.generate_snapshot().unwrap();
    assert_eq!(expected, result);
}

#[test]
fn test_exists() {
    let fs = InMemoryFs::new();
    let snapshot: VfsSnapshotNode = from_value(json!({
        "file": {
            "$data": [1, 2, 3]
        },
        "directory": {
            "entry1": {},
            "entry2": {
                "$data": []
            }
        }
    }))
    .unwrap();

    fs.apply_snapshot(&snapshot).unwrap();

    // It should return Ok(true) if a path points to a node
    assert_eq!(fs.exists("/file".into()).unwrap(), true);
    assert_eq!(fs.exists("/directory".into()).unwrap(), true);

    // It should return Ok(true) if a path cannot point to a node
    assert_eq!(fs.exists("/file1".into()).unwrap(), false);
    assert_eq!(fs.exists("/directory1".into()).unwrap(), false);

    // It should throw an error if the specified path's parent has no permission to read
    fs.set_permissions("/directory".into(), Permissions::WRITE)
        .unwrap();

    assert_io_err!(fs.exists("/directory/example.txt".into()), PermissionDenied);
}

#[test]
fn test_metadata() {
    let fs = InMemoryFs::new();
    let snapshot: VfsSnapshotNode = from_value(json!({
        "file": {
            "$data": [1, 2, 3]
        },
        "directory": {
            "entry1": {},
            "entry2": {
                "$data": []
            }
        }
    }))
    .unwrap();

    fs.apply_snapshot(&snapshot).unwrap();

    // It should provide node metadata for every file/directory
    let metadata = fs.metadata("/file".into()).unwrap();
    assert_eq!(metadata.mode, Permissions::READ_WRITE);
    assert_eq!(metadata.size, 3);
    assert_eq!(metadata.ty, NodeType::File);

    let metadata = fs.metadata("/directory".into()).unwrap();
    assert_eq!(metadata.mode, Permissions::READ_WRITE);
    assert_eq!(metadata.size, 0);
    assert_eq!(metadata.ty, NodeType::Directory);

    let metadata = fs.metadata("/directory/entry2".into()).unwrap();
    assert_eq!(metadata.mode, Permissions::READ_WRITE);
    assert_eq!(metadata.size, 0);
    assert_eq!(metadata.ty, NodeType::File);

    // How about a node permission change?
    fs.set_permissions("/file".into(), Permissions::READ)
        .unwrap();

    let metadata = fs.metadata("/file".into()).unwrap();
    assert_eq!(metadata.mode, Permissions::READ);
}

#[test]
fn test_read() {
    let fs = InMemoryFs::new();
    let snapshot: VfsSnapshotNode = VfsSnapshotNode::directory()
        .add("directory", VfsSnapshotNode::empty_dir())
        .file("example", b"This is a sample file.")
        .build();

    fs.apply_snapshot(&snapshot).unwrap();
    fs.write("/directory/sample".into(), b"Hello, World!")
        .unwrap();

    // It should return a byte vector upon successful read
    let content = fs.read("/example".into()).unwrap();
    assert_eq!(content, b"This is a sample file.");

    // It should return NotFound error if it has not found the file
    assert_io_err!(fs.read("/example2".into()), NotFound);

    // It should return NotFound error if one of the directory
    // components of the file path does not exist.
    assert_io_err!(fs.read("/etc/hostname".into()), NotFound);

    // It should return PermissionDenied if the user lacks permissions
    fs.set_permissions("/directory".into(), Permissions::WRITE)
        .unwrap();

    assert_io_err!(fs.read("/directory/sample".into()), PermissionDenied);

    fs.set_permissions("/directory".into(), Permissions::READ_WRITE)
        .unwrap();

    fs.set_permissions("/directory/sample".into(), Permissions::WRITE)
        .unwrap();

    assert_io_err!(fs.read("/directory/sample".into()), PermissionDenied);
}

#[test]
fn test_read_to_string() {
    let fs = InMemoryFs::new();
    let snapshot: VfsSnapshotNode = VfsSnapshotNode::directory()
        .add("directory", VfsSnapshotNode::empty_dir())
        .file("example", b"This is a sample file.")
        .file("invalid-utf8", &[0xFF, 0xFE])
        .build();

    fs.apply_snapshot(&snapshot).unwrap();
    fs.write("/directory/sample".into(), b"Hello, World!")
        .unwrap();

    // It should return a byte vector upon successful read
    let content = fs.read_to_string("/example".into()).unwrap();
    assert_eq!(content, "This is a sample file.");

    // It should return NotFound error if it has not found the file
    assert_io_err!(fs.read_to_string("/example2".into()), NotFound);

    // It should return NotFound error if one of the directory
    // components of the file path does not exist.
    assert_io_err!(fs.read_to_string("/etc/hostname".into()), NotFound);

    // It should return PermissionDenied if the user lacks permissions
    fs.set_permissions("/directory".into(), Permissions::WRITE)
        .unwrap();

    assert_io_err!(
        fs.read_to_string("/directory/sample".into()),
        PermissionDenied
    );

    fs.set_permissions("/directory".into(), Permissions::READ_WRITE)
        .unwrap();

    fs.set_permissions("/directory/sample".into(), Permissions::WRITE)
        .unwrap();

    assert_io_err!(
        fs.read_to_string("/directory/sample".into()),
        PermissionDenied
    );

    // It should return InvalidData for UTF-8 conversion error
    assert_io_err!(fs.read_to_string("/invalid-utf8".into()), InvalidData);
}
