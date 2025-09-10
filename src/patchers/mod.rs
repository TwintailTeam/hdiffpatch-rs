use std::fs::{remove_dir_all, File};
use std::io;
use std::io::{BufReader, Read, Seek};
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

pub struct KrPatcher {
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

        buf_reader.seek(io::SeekFrom::Start(diff.main_diff.new_data_offset))?;
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
            if self.in_buf.is_empty() {
                let read_bytes = self.file.read(&mut self.in_buf)?;
                if read_bytes == 0 { return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Unexpected end of input")); }
                self.in_buf.truncate(read_bytes);
            }

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

    pub(crate) fn patch(&mut self, source: &Path, dest: &Path, inplace: bool) -> bool {
        let mut old_files = Vec::new();
        for file in &self.diff.head_data.old_files {
            let full_path = source.join(&file.name);
            old_files.push(full_path);
        }

        // TODO: are we doing inplace patching??

        let covers = self.diff.main_diff.cover_buf.covers.clone();

        false
    }
}