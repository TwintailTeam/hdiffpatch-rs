use std::fs::create_dir_all;
use std::path::Path;
use crate::patchers::KrDiff;
use crate::utils::patch_krdir::KrPatchDir;

/*
WARNING: This shit is extremely cursed and is modification of standard HDiff format, it is not something you should use it can break and go to fuckshit anytime...
This only exists to support TwintailLauncher's use case and is very hacked to hell compared to actual standard HDiff patching part
HERE BE DRAGONS you are warned!!!
#FuckKuroGames btw
*/

impl KrDiff {
    pub fn new(source_path: String, diff_path: String, dest_path: String) -> Self {
        KrDiff { source_path, diff_path, dest_path, cache_size: 0 }
    }

    pub fn set_cache_size(&mut self, cache_size: usize) { self.cache_size = cache_size; }

    pub fn apply(&mut self) -> bool {
        match self.apply_inner() {
            Ok(()) => true,
            Err(e) => { eprintln!("[KrDiff::apply] Error: {}", e); false }
        }
    }

    fn apply_inner(&self) -> Result<(), Box<dyn std::error::Error>> {
        let src = Path::new(&self.source_path);
        let diffp = Path::new(&self.diff_path);

        let dst = std::path::PathBuf::from(&self.dest_path);
        if !src.exists() || !src.is_dir() { return Err(format!("[KrDiff] Source path {} does not exist or is not a directory", src.display()).into()); }
        if !diffp.exists() || !diffp.is_file() { return Err(format!("[KrDiff] Diff file {} does not exist", diffp.display()).into()); }
        if !dst.exists() { create_dir_all(&dst)?; }

        let patcher = KrPatchDir::new(self.diff_path.clone());
        patcher.patch(src.to_str().unwrap_or(""), dst.to_str().unwrap_or(""), None)?;
        Ok(())
    }
}
