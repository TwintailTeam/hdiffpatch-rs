use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::str::FromStr;

use crate::utils::compression_utils::get_clip_stream;
use crate::utils::parser::BinaryExtensions;
use crate::utils::structs::{CombinedStream, CompressionMode, NewFileCombinedStream};

/*
WARNING: This shit is extremely cursed and is modification of standard HDiff format, it is not something you should use it can break and go to fuckshit anytime...
This only exists to support TwintailLauncher's use case and is very hacked to hell compared to actual standard HDiff patching part
HERE BE DRAGONS you are warned!!!
#FuckKuroGames btw
*/

pub struct KrPatchDir {
    patch_path: String,
}

impl KrPatchDir {
    pub fn new(patch_path: String) -> Self {
        Self { patch_path }
    }

    pub fn patch(&self, input: &str, output: &str, write_bytes_cb: Option<Box<dyn FnMut(i64)>>) -> io::Result<()> {
        let base_input  = PathBuf::from(input);
        let base_output = PathBuf::from(output);

        let mut f = File::open(&self.patch_path)?;
        let hd19 = parse_hd19(&mut f)?;
        let hd13 = parse_hd13(&mut f)?;

        for dir in &hd19.head.new_directories {
            if !dir.is_empty() { fs::create_dir_all(base_output.join(dir.trim_end_matches('/')))?; }
        }

        for fe in &hd19.head.old_files {
            let full = base_input.join(&fe.path);
            if !full.exists() { return Err(io::Error::new(io::ErrorKind::NotFound, format!("[KrPatchDir] Old file not found: {}", full.display()))); }
            let actual = full.metadata()?.len();
            if actual != fe.size { return Err(io::Error::new(io::ErrorKind::InvalidData, format!("[KrPatchDir] Old file size mismatch for {}: expected {} bytes, got {}", full.display(), fe.size, actual))); }
        }

        for fe in &hd19.head.new_files {
            let full = base_output.join(&fe.path);
            if let Some(parent) = full.parent() { fs::create_dir_all(parent)?; }
            let file = File::options().read(true).write(true).create(true).truncate(true).open(&full)?;
            file.set_len(fe.size)?;
        }

        if hd19.head.old_files.is_empty() || hd19.head.new_files.is_empty() { return Ok(()); }

        let old_handles: Vec<File> = hd19.head.old_files.iter().map(|fe| File::open(base_input.join(&fe.path))).collect::<io::Result<_>>()?;
        let mut old_combined = CombinedStream::new(old_handles)?;

        let new_handles: Vec<NewFileCombinedStream> = hd19.head.new_files.iter().map(|fe| {
            let full = base_output.join(&fe.path);
            let file = File::options().read(true).write(true).open(&full)?;
            Ok(NewFileCombinedStream { file, size: fe.size })
        }).collect::<io::Result<_>>()?;
        let mut new_combined = CombinedStream::from_new_files(new_handles)?;

        let mut cb = write_bytes_cb;
        apply_patch(&hd13, hd19.old_ref_size, hd19.new_ref_size, &mut old_combined, &mut new_combined, &self.patch_path, &mut cb)?;
        new_combined.flush()?;
        Ok(())
    }
}

struct KrFileEntry {
    path: String,
    size: u64,
}

struct KrHead {
    old_files: Vec<KrFileEntry>,
    new_files: Vec<KrFileEntry>,
    new_directories: Vec<String>,
}

struct KrHd19 {
    comp_mode: CompressionMode,
    old_ref_size: u64,
    new_ref_size: u64,
    head: KrHead,
}

struct KrCover {
    old_pos_delta: i64,
    new_pos_gap: u64,
    length: u64,
}

struct KrHd13 {
    covers: Vec<KrCover>,
    new_data_size: u64,
    new_data_diff_offset: u64,
    new_data_diff_size: u64,
    new_data_diff_comp_size: u64,
    comp_mode: CompressionMode,
}

fn apply_patch(hd13: &KrHd13, old_ref_size: u64, new_ref_size: u64, old_combined: &mut CombinedStream, new_combined: &mut CombinedStream, patch_path: &str, write_bytes_cb: &mut Option<Box<dyn FnMut(i64)>>) -> io::Result<()> {
    let f_newdata = File::open(patch_path)?;
    let (mut new_data, _) = get_clip_stream(f_newdata, hd13.comp_mode, hd13.new_data_diff_offset, hd13.new_data_diff_size, hd13.new_data_diff_comp_size, false)?;

    let mut read_pos: i64 = 0;
    let mut write_pos: u64 = 0;
    let mut buf = vec![0u8; 64 * 1024];

    for cover in &hd13.covers {
        read_pos = read_pos.wrapping_add(cover.old_pos_delta);

        if old_ref_size > 0 {
            let sz = old_ref_size as i64;
            while read_pos > sz { read_pos -= sz; }
            while read_pos < 0  { read_pos += sz; }
        }

        if cover.new_pos_gap > 0 {
            copy_n(&mut *new_data, new_combined, cover.new_pos_gap as usize, &mut buf)?;
            write_pos += cover.new_pos_gap;
        }

        if cover.length > 0 {
            old_combined.seek(SeekFrom::Start(read_pos as u64))?;
            copy_n(old_combined, new_combined, cover.length as usize, &mut buf)?;
        }

        read_pos  = read_pos.wrapping_add(cover.length as i64);
        write_pos = write_pos.saturating_add(cover.length);
        if let Some(cb) = write_bytes_cb.as_mut() { cb(write_pos as i64); }
    }

    if write_pos < new_ref_size { copy_n(&mut *new_data, new_combined, (new_ref_size - write_pos) as usize, &mut buf)?; }
    Ok(())
}

fn copy_n(src: &mut dyn Read, dst: &mut dyn Write, mut n: usize, buf: &mut [u8]) -> io::Result<()> {
    while n > 0 {
        let to_read = buf.len().min(n);
        src.read_exact(&mut buf[..to_read])?;
        dst.write_all(&buf[..to_read])?;
        n -= to_read;
    }
    Ok(())
}

fn parse_hd19(reader: &mut (impl Read + Seek)) -> io::Result<KrHd19> {
    // "HDIFF19&<comp>&<checksum>\0<isOldDir><isNewDir>"
    let chunk_type = read_delim(reader, b'&', 10)?;
    if chunk_type != "HDIFF19" { return Err(io::Error::new(io::ErrorKind::InvalidData, format!("[KrPatchDir] Expected HDIFF19 chunk, got {:?}", chunk_type))); }
    let comp_str = read_delim(reader, b'&', 10)?;
    let _checksum_type = read_delim(reader, b'\0', 15)?;
    let _old_is_dir = reader.read_boolean()?;
    let _new_is_dir = reader.read_boolean()?;

    let old_path_count = reader.read_long_7bit()? as u64;
    let _old_path_sum_size = reader.read_long_7bit()? as u64;
    let new_path_count = reader.read_long_7bit()? as u64;
    let _new_path_sum_size = reader.read_long_7bit()? as u64;
    let old_ref_file_count = reader.read_long_7bit()? as u64;
    let old_ref_size = reader.read_long_7bit()? as u64;
    let new_ref_file_count = reader.read_long_7bit()? as u64;
    let new_ref_size = reader.read_long_7bit()? as u64;
    let _same_file_pair_count = reader.read_long_7bit()?;
    let _same_file_size = reader.read_long_7bit()?;
    let _new_execute_count = reader.read_long_7bit()?;
    let _private_reserved = reader.read_long_7bit()?;
    let private_extern_size = reader.read_long_7bit()? as u64;
    let extern_size = reader.read_long_7bit()? as u64;
    let head_data_size = reader.read_long_7bit()? as u64;
    let head_data_comp_size = reader.read_long_7bit()? as u64;
    let checksum_byte_size = reader.read_long_7bit()? as u64;

    skip_bytes(reader, checksum_byte_size * 4)?;

    let head = parse_hd19_head(reader, old_path_count, new_path_count, old_ref_file_count, new_ref_file_count, head_data_size, head_data_comp_size)?;
    skip_bytes(reader, private_extern_size)?;
    skip_bytes(reader, extern_size)?;

    let comp_mode = CompressionMode::from_str(&comp_str).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(KrHd19 { comp_mode, old_ref_size, new_ref_size, head })
}

fn parse_hd19_head(reader: &mut (impl Read + Seek), old_path_count:u64, new_path_count: u64, old_ref_file_count: u64, new_ref_file_count: u64, head_data_size: u64, head_data_comp_size: u64) -> io::Result<KrHead> {
    // Record start so we can seek to the exact end even if the decoder stops early.
    let section_start = reader.seek(SeekFrom::Current(0))?;
    let file_bytes = if head_data_comp_size > 0 { head_data_comp_size } else { head_data_size };

    let head = if head_data_comp_size > 0 {
        let window_log: u32 = if cfg!(target_pointer_width = "64") { 31 } else { 30 };
        // Stream directly into the decoder — no intermediate compressed or decompressed Vec.
        let mut dec = zstd::stream::read::Decoder::new(reader.by_ref().take(head_data_comp_size))?;
        dec.set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(window_log))?;
        parse_head_data_seq(&mut dec, old_path_count, new_path_count, old_ref_file_count, new_ref_file_count)?
    } else {
        let mut limited = reader.by_ref().take(head_data_size);
        parse_head_data_seq(&mut limited, old_path_count, new_path_count, old_ref_file_count, new_ref_file_count)?
    };

    // Guarantee reader sits exactly at the end of the section regardless of decoder internals.
    reader.seek(SeekFrom::Start(section_start + file_bytes))?;
    Ok(head)
}

fn parse_head_data_seq(reader: &mut impl Read, old_path_count: u64, new_path_count: u64, old_ref_file_count: u64, new_ref_file_count: u64) -> io::Result<KrHead> {
    let mut old_paths = Vec::with_capacity(old_path_count as usize);
    for _ in 0..old_path_count { old_paths.push(read_null_str(reader)?); }

    let mut new_paths = Vec::with_capacity(new_path_count as usize);
    for _ in 0..new_path_count { new_paths.push(read_null_str(reader)?); }

    let mut old_offsets = Vec::with_capacity(old_ref_file_count as usize);
    for _ in 0..old_ref_file_count { old_offsets.push(reader.read_long_7bit()? as u64); }
    let mut new_offsets = Vec::with_capacity(new_ref_file_count as usize);
    for _ in 0..new_ref_file_count { new_offsets.push(reader.read_long_7bit()? as u64); }

    let mut old_sizes = Vec::with_capacity(old_ref_file_count as usize);
    for _ in 0..old_ref_file_count { old_sizes.push(reader.read_long_7bit()? as u64); }

    let mut new_sizes = Vec::with_capacity(new_ref_file_count as usize);
    for _ in 0..new_ref_file_count { new_sizes.push(reader.read_long_7bit()? as u64); }

    // Unknown field present in KrDiff: one VarInt per new reference file.
    for _ in 0..new_ref_file_count { let _ = reader.read_long_7bit()?; }

    let (old_files, _old_dirs) = split_paths_with_offsets(&old_paths, &old_offsets, &old_sizes);
    let (new_files, new_directories) = split_paths_with_offsets(&new_paths, &new_offsets, &new_sizes);
    Ok(KrHead { old_files, new_files, new_directories })
}

fn split_paths_with_offsets(paths: &[String], offsets: &[u64], sizes: &[u64]) -> (Vec<KrFileEntry>, Vec<String>) {
    let mut files = Vec::new();
    let mut dirs  = Vec::new();

    if offsets.is_empty() {
        for path in paths { dirs.push(path.clone()); }
        return (files, dirs);
    }

    let mut offset_index: usize = 0;
    let mut next_file_index: u64 = offsets[0];

    for (i, path) in paths.iter().enumerate() {
        if i as u64 == next_file_index {
            if offset_index < offsets.len() - 1 {
                offset_index += 1;
                next_file_index += offsets[offset_index] + 1;
            }
            let size = sizes.get(files.len()).copied().unwrap_or(0);
            files.push(KrFileEntry { path: path.clone(), size });
        } else {
            dirs.push(path.clone());
        }
    }
    (files, dirs)
}

fn parse_hd13(reader: &mut (impl Read + Seek)) -> io::Result<KrHd13> {
    // "HDIFF13&<comp>\0"
    let chunk_type = read_delim(reader, b'&', 10)?;
    if chunk_type != "HDIFF13" { return Err(io::Error::new(io::ErrorKind::InvalidData, format!("[KrPatchDir] Expected HDIFF13 chunk, got {:?}", chunk_type))); }
    let comp_str = read_delim(reader, b'\0', 10)?;

    let new_data_size = reader.read_long_7bit()? as u64;
    let _old_data_size = reader.read_long_7bit()? as u64;
    let cover_count = reader.read_long_7bit()? as u64;
    let cover_buf_size = reader.read_long_7bit()? as u64;
    let comp_cover_buf_size = reader.read_long_7bit()? as u64;
    let rle_ctrl_buf_size = reader.read_long_7bit()? as u64;
    let comp_rle_ctrl_buf_size = reader.read_long_7bit()? as u64;
    let rle_code_buf_size = reader.read_long_7bit()? as u64;
    let comp_rle_code_buf_size = reader.read_long_7bit()? as u64;
    let new_data_diff_size = reader.read_long_7bit()? as u64;
    let new_data_diff_comp_size = reader.read_long_7bit()? as u64;

    let cover_buf_start = reader.seek(SeekFrom::Current(0))?;
    let covers = read_covers(reader, cover_count, cover_buf_size, comp_cover_buf_size)?;

    let cover_file_bytes    = if comp_cover_buf_size    > 0 { comp_cover_buf_size    } else { cover_buf_size    };
    let rle_ctrl_file_bytes = if comp_rle_ctrl_buf_size > 0 { comp_rle_ctrl_buf_size } else { rle_ctrl_buf_size };
    let rle_code_file_bytes = if comp_rle_code_buf_size > 0 { comp_rle_code_buf_size } else { rle_code_buf_size };
    let new_data_diff_offset = cover_buf_start + cover_file_bytes + rle_ctrl_file_bytes + rle_code_file_bytes;
    let comp_mode = CompressionMode::from_str(&comp_str).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(KrHd13 {
        covers,
        new_data_size,
        new_data_diff_offset,
        new_data_diff_size,
        new_data_diff_comp_size,
        comp_mode,
    })
}

fn read_covers(reader: &mut impl Read, cover_count: u64, cover_buf_size: u64, comp_cover_buf_size: u64) -> io::Result<Vec<KrCover>> {
    let mut covers = Vec::with_capacity(cover_count as usize);

    if comp_cover_buf_size > 0 {
        let window_log: u32 = if cfg!(target_pointer_width = "64") { 31 } else { 30 };
        // Stream directly into the decoder — no intermediate compressed or decompressed Vec.
        let mut dec = zstd::stream::read::Decoder::new(reader.by_ref().take(comp_cover_buf_size))?;
        dec.set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(window_log))?;
        parse_covers_seq(&mut dec, cover_count, &mut covers)?;
    } else {
        let mut limited = reader.by_ref().take(cover_buf_size);
        parse_covers_seq(&mut limited, cover_count, &mut covers)?;
    }

    Ok(covers)
}

fn parse_covers_seq(reader: &mut impl Read, cover_count: u64, covers: &mut Vec<KrCover>) -> io::Result<()> {
    for _ in 0..cover_count {
        let mut first = [0u8; 1];
        reader.read_exact(&mut first)?;
        let p_sign  = first[0];
        let sign    = (p_sign >> 7) != 0;
        let abs_val = reader.read_long_7bit_tagged(1, p_sign)?;
        let old_pos_delta = if sign { -abs_val } else { abs_val };
        let new_pos_gap = reader.read_long_7bit()? as u64;
        let length = reader.read_long_7bit()? as u64;
        covers.push(KrCover { old_pos_delta, new_pos_gap, length });
    }
    Ok(())
}

fn read_delim(reader: &mut impl Read, delim: u8, limit: usize) -> io::Result<String> {
    let mut buf  = Vec::with_capacity(16);
    let mut byte = [0u8; 1];
    loop {
        reader.read_exact(&mut byte)?;
        if byte[0] == delim { break; }
        buf.push(byte[0]);
        if buf.len() >= limit { break; }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn read_null_str(reader: &mut impl Read) -> io::Result<String> {
    read_delim(reader, b'\0', 255)
}

fn skip_bytes(reader: &mut (impl Read + Seek), n: u64) -> io::Result<()> {
    if n > 0 { reader.seek(SeekFrom::Current(n as i64))?; }
    Ok(())
}
