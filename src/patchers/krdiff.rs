use std::fs::{create_dir_all};
use std::path::Path;
use crate::patchers::{KrDiff, KrPatcher};
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
        if let Some(d) = &self.dest_path { dst = std::path::PathBuf::from(d); } else { dst = std::path::PathBuf::from(src).join("tmp"); }

        if !src.exists() || !src.is_dir() { eprintln!("Source path {} does not exist!", src.display()); return false; }
        if !diffp.exists() && diffp.is_file() { eprintln!("Diff {} does not exist!", diffp.display()); return false; }
        if !dst.is_dir() { eprintln!("Destination path {} must be a directory!", dst.display()); return false; }
        if !dst.exists() { create_dir_all(dst.clone()).unwrap(); }

        let mut p = Parser::from_path(diffp.to_str().unwrap(), None);
        let mut dir_diff = DirDiff::parse(&mut p);

        // RLE stuff
        if dir_diff.main_diff.compressed_rle_code_buf_size.0 > 0 { p.read_bytes::<u8>(dir_diff.main_diff.compressed_rle_code_buf_size.0 as usize); } else { p.read_bytes::<u8>(dir_diff.main_diff.rle_code_buf_size.0 as usize); }
        if dir_diff.main_diff.compressed_rle_ctrl_buf_size.0 > 0 { p.read_bytes::<u8>(dir_diff.main_diff.compressed_rle_ctrl_buf_size.0 as usize); } else { p.read_bytes::<u8>(dir_diff.main_diff.rle_ctrl_buf_size.0 as usize); }
        dir_diff.main_diff.new_data_offset = p.position();
        #[cfg(debug_assertions)] { println!("Parsing KrDiff file with {:?} new folder(s) and {:?} new file(s) | Old stats are {:?} folder(s) and {:?} file(s)", dir_diff.head_data.new_dirs.len(), dir_diff.head_data.new_files.len(), dir_diff.head_data.old_dirs.len(), dir_diff.head_data.old_files.len()); }

        let mut patcher = KrPatcher::new(dir_diff, diffp).unwrap();
        patcher.patch(src, dst.as_path())
    }
}
