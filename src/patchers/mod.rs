pub mod krdiff;
pub mod hdiff;

pub struct KrDiff {
    source_path: String,
    diff_path: String,
    dest_path: String,
    cache_size: usize,
}

pub struct HDiff {
    source_path: String,
    diff_path: String,
    dest_path: String,
    cache_size: usize,
}