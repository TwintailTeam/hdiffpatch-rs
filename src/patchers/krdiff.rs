use std::fs::{create_dir_all};
use std::path::Path;
use crate::patchers::{KrDiff};
use crate::utils::{DirDiff};
use crate::utils::parser::Parser;

impl KrDiff {
    pub fn new(source_path: String, kr_diff_path: String, dest_path: Option<String>) -> Self {
        KrDiff { source_path, kr_diff_path, dest_path, cache_size: 0 }
    }

    pub fn set_cache_size(&mut self, cache_size: usize) { self.cache_size = cache_size; }

    pub fn apply(&mut self) -> bool {
        let src = Path::new(&self.source_path);
        let diffp = Path::new(&self.kr_diff_path);
        let dst: std::path::PathBuf;
        if let Some(d) = &self.dest_path {
            dst = std::path::PathBuf::from(d);
        } else {
            dst = std::path::PathBuf::from(src);
        }

        if !src.exists() || !src.is_dir() { panic!("Source path {} does not exist!", src.display()); }
        if !diffp.exists() && diffp.is_file() { panic!("Diff {} does not exist!", diffp.display()); }
        if !dst.is_dir() { panic!("Destination path {} must be a directory!", dst.display()); }

        if !dst.exists() { create_dir_all(dst.clone()).unwrap(); }

        let mut p = Parser::from_path(diffp.to_str().unwrap(), None);
        let dir_diff = DirDiff::parse(&mut p);

        true
    }
}
