use std::fs::{remove_dir_all, File};
use std::{fs, io};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use zstd::stream::Decoder;
use crate::utils;
use crate::utils::{merge_dirs_recursive, DirDiff};

pub mod krdiff;
pub mod hdiff;

pub struct KrDiff {
    source_path: String,
    kr_diff_path: String,
    dest_path: Option<String>,
    cache_size: usize,
}

pub struct HDiff {
    source_path: String,
    diff_path: String,
    dest_path: String,
    cache_size: usize,
}

pub(crate) struct KrPatcher {
    diff: DirDiff,
    file: BufReader<File>,
    current_index: u64,
    cur_file: Option<utils::File>,
    decoder: Decoder<'static, BufReader<File>>,
    in_buf: Vec<u8>,
}

impl KrPatcher {
    pub(crate) fn new(diff: DirDiff, diff_file: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::open(&diff_file)?;
        let mut buf_reader = BufReader::new(file);
        buf_reader.seek(SeekFrom::Start(diff.main_diff.new_data_offset))?;

        let decoder = Decoder::with_buffer(buf_reader)?;
        let chunk_size = 128 * 1024;

        let file2 = File::open(&diff_file)?;
        let buf_reader2 = BufReader::new(file2);
        Ok(Self { diff, file: buf_reader2, current_index: 0, cur_file: None, decoder, in_buf: vec![0u8; chunk_size] })
    }

    pub(crate) fn read(&mut self, buf: &mut [u8]) -> io::Result<u64> {
        if buf.is_empty() { return Ok(0); }

        let mut output_pos = 0;
        while output_pos < buf.len() {
            let read_bytes = self.decoder.read(&mut buf[output_pos..])?;
            if read_bytes == 0 { return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Decompressed frame too small for requested size")); }
            output_pos += read_bytes;
        }
        self.current_index += buf.len() as u64;
        Ok(buf.len() as u64)
    }

    pub(crate) fn merge_dirs(&self, a: &Path, b: &Path) {
        merge_dirs_recursive(a, b).unwrap();
        remove_dir_all(b).unwrap();
    }

    pub(crate) fn patch(&mut self, source: &Path, dest: &Path) -> bool {
        let mut old_files = Vec::new();
        for file in self.diff.head_data.old_files.clone() {
            let full_path = source.join(&file.name);
            old_files.push(full_path);
        }

        for nd in self.diff.head_data.new_dirs.clone() {
            let full_path = dest.join(&nd.name);
            fs::create_dir_all(full_path).unwrap();
        }

        let mut covers = self.diff.main_diff.cover_buf.covers.clone();
        let mut cover_idx = 0;
        let mut old_pos = 0;
        let mut to_read = covers[0].new_pos;
        let mut written: u64 = 0;

        let new_files = self.diff.head_data.new_files.clone();
        for (i, cur_file) in new_files.iter().enumerate() {
            let destination_file = dest.join(&cur_file.name);
            #[cfg(debug_assertions)] { println!("[{}/{}] Patching {:?}", i + 1, &self.diff.head_data.new_files.len(), destination_file); }
            let mut f = File::create(&destination_file).unwrap();
            f.set_len(cur_file.file_size).unwrap();

            let old_file_path = &old_files.get(i).unwrap();
            let mut reader = File::open(old_file_path).unwrap();

            while written < cur_file.file_size {
                let remaining = cur_file.file_size - written;
                if to_read == 0 && cover_idx < covers.len() {
                    let cov = &mut covers[cover_idx];
                    old_pos += cov.old_pos;
                    let to_write = cov.length.min(remaining);
                    let mut vv = vec![0u8; to_write as usize];

                    if !reader.seek(SeekFrom::Start(old_pos as u64)).is_ok() { eprintln!("Error while seeking in vfs"); return false; }
                    if !reader.read(&mut vv).is_ok() { eprintln!("Unexpected EOF!"); return false; }
                    assert_eq!(vv.len(), to_write as usize);
                    f.write_all(&vv).unwrap();
                    written += to_write;
                    old_pos += to_write as i64;
                    if remaining == to_write && remaining != cov.length {
                        to_read = 0;
                        cov.length -= to_write;
                        cov.old_pos = 0;
                        cov.new_pos = 0;
                    } else {
                        if cover_idx + 1 < covers.len() { to_read = covers[cover_idx + 1].new_pos; }
                        cover_idx += 1;
                    }
                } else {
                    let mut to_write = remaining;
                    if cover_idx < covers.len() { to_write = remaining.min(to_read); }
                    let mut v = vec![0u8; to_write as usize];
                    self.read(&mut v).unwrap();
                    f.write_all(&v).unwrap();
                    to_read -= to_write;
                    written += to_write;
                }
            }
            written = 0;
            #[cfg(debug_assertions)] { println!("Successfully patched {:?} [{}/{}]", dest.join(&cur_file.name), i + 1, &self.diff.head_data.new_files.len()); }
        }
        // merge if inplace
        #[cfg(debug_assertions)] { println!("Everything patched with success"); }
        true
    }
}