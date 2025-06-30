use fuse::{Filesystem, Request, ReplyAttr, ReplyEntry, ReplyDirectory, ReplyData, FileAttr, FileType};
const ROOT_INO: u64 = 1;

fn default_time() -> Timespec {
    Timespec { sec: 1_600_000_000, nsec: 0 }
}
pub struct OneDriveFs ;
impl Filesystem for OneDriveFs {
    fn getattr(&self, req: &Request, ino: u64, fh: Option<u64>) -> Result<ReplyAttr, Error> {
        let attr = FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: default_time(),
            mtime: default_time(),
            ctime: default_time(),
            crtime: default_time(),
            kind: FileType::RegularFile,
            perm: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 0,
            padding: 0,
            fstype: 0,
            lock_owner: 0,
        };
        Ok(ReplyAttr::from(attr))
    }
    fn readdir(&self, req: &Request, ino: u64, fh: Option<u64>, offset: u64) -> Result<ReplyDirectory, Error> { 
        
        let mut entries = Vec::new();
        if ino == ROOT_INO {
            entries.push(ReplyEntry::new(".", FileType::Directory));
            entries.push(ReplyEntry::new("..", FileType::Directory));
        }
        
        Ok(ReplyDirectory::from(entries))
    }
    
}