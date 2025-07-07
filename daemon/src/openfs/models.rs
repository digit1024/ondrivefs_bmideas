use std::collections::HashMap;

use fuser::FileType;
use rand::Rng;
#[derive(Clone, Debug)]
pub struct DirEntry {
    pub ino: u64,
    pub offset: i64,
    pub kind: FileType,
    pub name: String,
}

impl DirEntry {
    pub fn new(ino: u64, offset: i64, kind: FileType, name: String) -> Self {
        Self {
            ino,
            offset,
            kind,
            name,
        }
    }
}
pub struct DirHanldeManager {
    pub dir_handles: HashMap<u64, Vec<DirEntry>>,
}

impl DirHanldeManager {
    pub fn new() -> Self {
        Self {
            dir_handles: HashMap::new(),
        }
    }

    pub fn new_dir_handle(&mut self) -> u64 {
        //generate random ino
        let ino = rand::thread_rng().gen_range(1..=u64::MAX);
        //check if ino is already in use
        if self.dir_handles.contains_key(&ino) {
            return self.new_dir_handle();
        }
        let entries = Vec::new();
        self.dir_handles.insert(ino, entries);
        ino
    }

    #[allow(dead_code)]
    pub fn add_dir_handle(&mut self, ino: u64, entries: Vec<DirEntry>) {
        self.dir_handles.insert(ino, entries);
    }

    pub fn get_dir_handle(&self, ino: u64) -> Option<&Vec<DirEntry>> {
        self.dir_handles.get(&ino)
    }
    pub fn append_to_handle(&mut self, ino: u64, entry: DirEntry) {
        self.dir_handles.get_mut(&ino).unwrap().push(entry.clone());
    }

    pub fn remove_dir_handle(&mut self, ino: u64) {
        self.dir_handles.remove(&ino);
    }
}
