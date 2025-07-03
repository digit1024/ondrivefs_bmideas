use anyhow::Context;
use std::os::unix::fs::MetadataExt;
use std::path::Path; // Unix-specific trait

pub fn path_to_inode(cache_path: &Path) -> u64 {
    if cache_path.is_file() || cache_path.is_dir() {
        return cache_path
            .metadata()
            .context("Getting metadata failed")
            .unwrap()
            .ino();
    }
    return 0;
}
