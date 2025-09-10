use crate::patchers::{HDiff};

impl HDiff {
    pub fn new(source_path: String, diff_path: String, dest_path: String) -> Self {
        HDiff { source_path, diff_path, dest_path, cache_size: 0 }
    }

    pub fn set_cache_size(&mut self, cache_size: usize) { self.cache_size = cache_size; }
    
    pub fn apply(&mut self) -> bool {
        true
    }
}