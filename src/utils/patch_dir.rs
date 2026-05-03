use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::utils::compression_utils::get_clip_stream;
use crate::utils::header::Header;
use crate::utils::parser::BinaryExtensions;
use crate::utils::structs::PatchCoreImpl;
use crate::utils::structs::{
    CombinedStream, CompressionMode, DataReferenceInfo, DirectoryReferencePair,
    HeaderInfo, NewFileCombinedStream, PatchCore,
};

pub(crate) struct PatchDir {
    header_info: HeaderInfo,
    reference_info: DataReferenceInfo,
    patch_path: String,
}

impl PatchDir {
    pub fn new(header_info: HeaderInfo, reference_info: DataReferenceInfo, patch_path: String) -> Self {
        Self { header_info, reference_info, patch_path }
    }

    pub fn patch(&mut self, input: &str, output: &str, write_bytes_cb: Option<Box<dyn FnMut(i64)>>) -> std::io::Result<()> {
        let base_input  = PathBuf::from(input);
        let base_output = PathBuf::from(output);
        let padding: u64 = match self.header_info.comp_mode { CompressionMode::Zlib => 1, _ => 0};

        let ri = &self.reference_info;
        let header_padding  = if ri.head_data_compressed_size > 0 { padding } else { 0 };
        let head_comp_size  = (ri.head_data_compressed_size as u64).saturating_sub(header_padding);

        let head_file = File::open(&self.patch_path)?;
        let (mut head_stream, _) = get_clip_stream(head_file, self.header_info.comp_mode, ri.head_data_offset as u64 + header_padding, ri.head_data_size as u64, head_comp_size, true)?;
        let dir_data = self.init_dir_patcher(&mut *head_stream)?;

        let old_files = Self::get_ref_old_streams(&dir_data, &base_input)?;
        let new_files = Self::get_ref_new_streams(&dir_data, &base_output)?;

        if self.header_info.is_single_compressed_diff { return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "[PatchDir::patch] Single-compressed dir patches are not supported")); }

        let mut patch_for_inner = File::open(&self.patch_path)?;
        patch_for_inner.seek(SeekFrom::Start(self.reference_info.hdiff_data_offset as u64))?;
        let mut dummy_ref = DataReferenceInfo::default();
        Header::try_parse_header_info(&mut patch_for_inner, "", &mut self.header_info, &mut dummy_ref).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        // Padding may have changed if inner patch uses a different comp mode.
        let padding: u64 = match self.header_info.comp_mode { CompressionMode::Zlib => 1, _ => 0 };
        let mut old_combined = CombinedStream::new(old_files)?;
        let mut new_combined = CombinedStream::from_new_files(new_files)?;

        if old_combined.length() as i64 != self.header_info.old_data_size { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("[PatchDir::patch] Old size mismatch: expected {} bytes, got {} bytes", self.header_info.old_data_size, old_combined.length()))); }
        let mut core = PatchCoreImpl::new(self.header_info.new_data_size, base_input, base_output, write_bytes_cb);
        core.set_directory_reference_pair(dir_data);
        self.start_patch_routine(&mut old_combined, &mut new_combined, &mut core, padding)?;
        new_combined.flush()?;
        Ok(())
    }

    fn start_patch_routine(&self, old_stream: &mut CombinedStream, new_stream: &mut CombinedStream, core: &mut PatchCoreImpl, padding: u64) -> std::io::Result<()> {
        let hi = &self.header_info;
        let ci = &hi.chunk_info;

        let f0 = File::open(&self.patch_path)?;
        let f1 = File::open(&self.patch_path)?;
        let f2 = File::open(&self.patch_path)?;
        let f3 = File::open(&self.patch_path)?;

        // head_end_pos is the absolute offset in the patch file where the clips begin.
        let mut offset = ci.head_end_pos as u64;

        // clip[0]: cover_buf (always buffered in memory)
        let cover_padding = if ci.compress_cover_buf_size > 0 { padding } else { 0 };
        let (clip0, len0) = get_clip_stream(f0, hi.comp_mode, offset + cover_padding, ci.cover_buf_size as u64, ci.compress_cover_buf_size as u64, true)?;
        offset += len0;

        // clip[1]: rle_ctrl_buf (buffered)
        let rle_ctrl_padding = if ci.compress_rle_ctrl_buf_size > 0 { padding } else { 0 };
        let (clip1, len1) = get_clip_stream(f1, hi.comp_mode, offset + rle_ctrl_padding, ci.rle_ctrl_buf_size as u64, ci.compress_rle_ctrl_buf_size as u64, true)?;
        offset += len1;

        // clip[2]: rle_code_buf (buffered)
        let rle_code_padding = if ci.compress_rle_code_buf_size > 0 { padding } else { 0 };
        let (clip2, len2) = get_clip_stream(f2, hi.comp_mode, offset + rle_code_padding, ci.rle_code_buf_size as u64, ci.compress_rle_code_buf_size as u64, true)?;
        offset += len2;

        // clip[3]: new_data_diff (lazy — can be very large)
        let new_data_diff_padding = if ci.compress_new_data_diff_size > 0 { padding } else { 0 };
        let comp_diff_size = (ci.compress_new_data_diff_size as u64).saturating_sub(padding);
        let (clip3, _) = get_clip_stream(f3, hi.comp_mode, offset + new_data_diff_padding, ci.new_data_diff_size as u64, comp_diff_size, false)?;
        let mut clips: [Box<dyn Read>; 4] = [clip0, clip1, clip2, clip3];
        core.uncover_buffer_clips_stream(&mut clips, old_stream, new_stream, hi);
        Ok(())
    }

    fn init_dir_patcher(&self, mut reader: &mut dyn Read) -> std::io::Result<DirectoryReferencePair> {
        let ri = &self.reference_info;
        // Old and new path lists (null-separated strings packed into a fixed-size buffer).
        let old_utf8_path_list = reader.get_paths_from_stream(ri.input_sum_size as usize, ri.input_dir_count as usize)?;
        let new_utf8_path_list = reader.get_paths_from_stream(ri.output_sum_size as usize, ri.output_dir_count as usize)?;
        // Reference index lists (delta-encoded, validated against path count).
        let old_ref_list = reader.get_longs_from_stream(ri.input_ref_file_count as usize, Some(ri.input_dir_count))?;
        let new_ref_list = reader.get_longs_from_stream(ri.output_ref_file_count as usize, Some(ri.output_dir_count))?;
        // New-file sizes (raw absolute values, not delta-encoded).
        let new_ref_size_list = reader.get_longs_from_stream_absolute(ri.output_ref_file_count as usize)?;
        // Same-file pairs (new-old index pairs, delta-encoded with sign bit).
        let data_same_pair_list = reader.get_pair_index_reference_from_stream(ri.same_file_pair_count as usize, ri.output_dir_count, ri.input_dir_count)?;
        // New-execute list (delta-encoded indices into new path list).
        let new_execute_list = reader.get_longs_from_stream(ri.new_execute_count as usize, Some(ri.output_dir_count))?;
        Ok(DirectoryReferencePair {
            old_utf8_path_list,
            new_utf8_path_list,
            old_ref_list,
            new_ref_list,
            new_ref_size_list,
            data_same_pair_list,
            new_execute_list,
        })
    }

    fn get_ref_old_streams(dir_data: &DirectoryReferencePair, base_input: &Path) -> std::io::Result<Vec<File>> {
        let mut streams = Vec::with_capacity(dir_data.old_ref_list.len());
        for &ref_idx in &dir_data.old_ref_list {
            let path     = &dir_data.old_utf8_path_list[ref_idx as usize];
            let full_path = base_input.join(path);
            streams.push(File::open(&full_path)?);
        }
        Ok(streams)
    }

    fn get_ref_new_streams(dir_data: &DirectoryReferencePair, base_output: &Path) -> std::io::Result<Vec<NewFileCombinedStream>> {
        let mut streams = Vec::with_capacity(dir_data.new_ref_list.len());
        for (i, &ref_idx) in dir_data.new_ref_list.iter().enumerate() {
            let path      = &dir_data.new_utf8_path_list[ref_idx as usize];
            let full_path  = base_output.join(path);
            if let Some(parent) = full_path.parent() { fs::create_dir_all(parent)?; }
            let file = File::options().read(true).write(true).create(true).truncate(true).open(&full_path)?;
            streams.push(NewFileCombinedStream { file, size: dir_data.new_ref_size_list[i] as u64, });
        }
        Ok(streams)
    }
}
