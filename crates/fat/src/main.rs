use fat_imfs::InMemoryFs;
use fat_vfs::VfsSnapshotNode;
use std::io;

fn main() -> io::Result<()> {
    let snapshot = VfsSnapshotNode::directory()
        .add(
            "etc",
            VfsSnapshotNode::directory()
                .file("hostname", "stuff")
                .file("os-release", "Linux")
                .build(),
        )
        .add(
            "home",
            VfsSnapshotNode::directory()
                .add("memo", VfsSnapshotNode::empty_dir())
                .build(),
        )
        .build();

    let fs = InMemoryFs::new();
    fs.apply_snapshot(&snapshot)?;

    let snapshot = fs.generate_snapshot()?;
    println!("{snapshot:?}");

    Ok(())
}
