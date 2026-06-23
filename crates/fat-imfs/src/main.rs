use fat_imfs::InMemoryFs;
use fat_vfs::FileSystem;

fn main() {
    let imfs = InMemoryFs::new();
    imfs.create_dir_all("/home/memo/.config".into()).unwrap();
    // imfs.set_permissions("/home".into(), Permissions::READ)
    //     .unwrap();

    // imfs.set_permissions_recursive("/".into(), Permissions::READ_WRITE)
    //     .unwrap();

    println!("{imfs:#?}");
}
