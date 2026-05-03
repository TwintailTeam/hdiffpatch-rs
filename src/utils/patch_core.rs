use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use crate::utils::parser::{read_long_7bit_from_slice, BinaryExtensions};
use crate::utils::structs::{
    CoverHeader, DirectoryReferencePair, HeaderInfo, PatchCore, PatchCoreImpl,
    RleRefClip, SeekableRead,
};

const K_SIGN_TAG_BIT: u8  = 1;
const K_BYTE_RLE_TYPE: u8 = 2;
const MAX_MEM_BUFFER_LEN: i64 = 7 << 20;  // 7 MiB: switch to in-memory cover parsing below this
const MAX_MEM_BUFFER_LIMIT: usize = 10 << 20; // 10 MiB: flush cache output above this
const MAX_ARRAY_POOL_LEN: usize = 4 << 20;  // 4 MiB: shared read buffer
const MAX_ARRAY_POOL_SECOND_OFFSET: usize = MAX_ARRAY_POOL_LEN / 2;

impl PatchCore for PatchCoreImpl {
    fn set_directory_reference_pair(&mut self, pair: DirectoryReferencePair) {
        self.dir_reference_pair = Some(pair);
    }

    fn set_size_to_be_patched(&mut self, size_to_be_patched: i64, size_to_patch: i64) {
        self.size_to_be_patched = size_to_be_patched;
        self.size_patched = size_to_patch;
    }

    fn uncover_buffer_clips_stream(&mut self, clips: &mut [Box<dyn Read>], input_stream: &mut dyn SeekableRead, output_stream: &mut dyn Write, header_info: &HeaderInfo) {
        self.write_cover_stream_to_output(clips, input_stream, output_stream, header_info.chunk_info.cover_count, header_info.chunk_info.cover_buf_size, header_info.new_data_size);
    }
}

impl PatchCoreImpl {
    pub fn new(size_to_be_patched: i64, input_path: std::path::PathBuf, output_path: std::path::PathBuf, write_bytes_callback: Option<Box<dyn FnMut(i64)>>) -> Self {
        Self {
            size_to_be_patched,
            size_patched: 0,
            path_input: input_path,
            path_output: output_path,
            dir_reference_pair: None,
            write_bytes_callback,
        }
    }

    pub fn enumerate_cover_headers(mut cover_reader: &mut dyn Read, cover_size: i64, cover_count: i64) -> Vec<CoverHeader> {
        let mut headers = Vec::with_capacity(cover_count as usize);
        let mut last_old_pos_back = 0i64;
        let mut last_new_pos_back = 0i64;
        let mut remaining = cover_count;

        if cover_size < MAX_MEM_BUFFER_LEN {
            let mut buffer = vec![0u8; cover_size as usize];
            cover_reader.read_exact(&mut buffer).expect("failed to read cover buffer");

            let mut offset = 0usize;
            while remaining > 0 {
                remaining -= 1;

                let old_pos_back = last_old_pos_back;
                let new_pos_back = last_new_pos_back;
                let p_sign = buffer[offset];
                offset += 1;

                let inc_old_pos_sign = p_sign >> (8 - K_SIGN_TAG_BIT);
                let inc_old_pos = read_long_7bit_from_slice(&buffer, &mut offset, K_SIGN_TAG_BIT, p_sign);
                let old_pos = if inc_old_pos_sign == 0 { old_pos_back + inc_old_pos } else { old_pos_back - inc_old_pos };

                let copy_length  = read_long_7bit_from_slice(&buffer, &mut offset, 0, 0);
                let cover_length = read_long_7bit_from_slice(&buffer, &mut offset, 0, 0);
                let new_pos_back = new_pos_back + copy_length;
                last_old_pos_back = old_pos + cover_length;
                last_new_pos_back = new_pos_back + cover_length;
                headers.push(CoverHeader::new(old_pos, new_pos_back, cover_length, remaining));
            }
        } else {
            while remaining > 0 {
                remaining -= 1;

                let old_pos_back = last_old_pos_back;
                let new_pos_back = last_new_pos_back;
                let mut p_sign_buf = [0u8; 1];
                cover_reader.read_exact(&mut p_sign_buf).expect("failed to read pSign from cover stream");
                let p_sign = p_sign_buf[0];

                let inc_old_pos_sign = p_sign >> (8 - K_SIGN_TAG_BIT);
                let inc_old_pos = cover_reader.read_long_7bit_tagged(K_SIGN_TAG_BIT, p_sign).expect("failed to read incOldPos");
                let old_pos = if inc_old_pos_sign == 0 { old_pos_back + inc_old_pos } else { old_pos_back - inc_old_pos };

                let copy_length  = cover_reader.read_long_7bit().expect("failed to read copyLength");
                let cover_length = cover_reader.read_long_7bit().expect("failed to read coverLength");
                let new_pos_back = new_pos_back + copy_length;
                last_old_pos_back = old_pos + cover_length;
                last_new_pos_back = new_pos_back + cover_length;
                headers.push(CoverHeader::new(old_pos, new_pos_back, cover_length, remaining));
            }
        }
        headers
    }

    fn write_cover_stream_to_output(&mut self, clips: &mut [Box<dyn Read>], input_stream: &mut dyn SeekableRead, output_stream: &mut dyn Write, cover_count: i64, cover_size: i64, new_data_size: i64) {
        let mut shared_buffer = vec![0u8; MAX_ARRAY_POOL_LEN];
        let mut cache = Cursor::new(Vec::<u8>::new());

        self.run_copy_similar_files_routine();
        let mut new_pos_back = 0i64;
        let mut rle_struct = RleRefClip::default();
        let (left, right) = clips.split_at_mut(2);
        let headers = Self::enumerate_cover_headers(&mut *left[0], cover_size, cover_count);

        for cover in &headers {
            if new_pos_back < cover.new_pos {
                let copy_length = cover.new_pos - new_pos_back;
                Self::tbytes_copy_stream_from_old_clip(&mut cache, &mut *right[1], copy_length, &mut shared_buffer);
                Self::tbytes_determine_rle_type(&mut rle_struct, &mut cache, copy_length, &mut shared_buffer, &mut *left[1], &mut *right[0]);
            }

            Self::tbytes_copy_old_clip_patch(&mut cache, input_stream, &mut rle_struct, cover.old_pos, cover.cover_length, &mut shared_buffer, &mut *left[1], &mut *right[0]);
            new_pos_back = cover.new_pos + cover.cover_length;
            if cache.get_ref().len() > MAX_MEM_BUFFER_LIMIT || cover.next_cover_index == 0 { Self::write_cache_to_output(&mut cache, output_stream, &mut self.write_bytes_callback); }
        }

        if new_pos_back < new_data_size {
            let copy_length = new_data_size - new_pos_back;
            Self::tbytes_copy_stream_from_old_clip(&mut cache, &mut *right[1], copy_length, &mut shared_buffer);
            Self::tbytes_determine_rle_type(&mut rle_struct, &mut cache, copy_length, &mut shared_buffer, &mut *left[1], &mut *right[0]);
            Self::write_cache_to_output(&mut cache, output_stream, &mut self.write_bytes_callback);
        }
    }

    fn write_cache_to_output(cache: &mut Cursor<Vec<u8>>, output: &mut dyn Write, callback: &mut Option<Box<dyn FnMut(i64)>>) {
        let data = cache.get_ref();
        let written = data.len() as i64;
        output.write_all(data).expect("failed to write cache to output");
        cache.get_mut().clear();
        cache.set_position(0);
        if let Some(cb) = callback.as_mut() { cb(written); }
    }

    fn tbytes_copy_old_clip_patch(out_cache: &mut Cursor<Vec<u8>>, input_stream: &mut dyn SeekableRead, rle_loader: &mut RleRefClip, old_pos: i64, add_length: i64, shared_buffer: &mut [u8], rle_ctrl_stream: &mut dyn Read, rle_code_stream: &mut dyn Read) {
        let last_pos = out_cache.position();
        input_stream.seek(SeekFrom::Start(old_pos as u64)).expect("failed to seek input_stream");
        Self::tbytes_copy_stream_inner(input_stream, out_cache, shared_buffer, add_length as usize);
        out_cache.seek(SeekFrom::Start(last_pos)).expect("failed to restore cache position");
        Self::tbytes_determine_rle_type(rle_loader, out_cache, add_length, shared_buffer, rle_ctrl_stream, rle_code_stream);
    }

    pub(crate) fn tbytes_copy_stream_from_old_clip(out_cache: &mut Cursor<Vec<u8>>, copy_reader: &mut dyn Read, copy_length: i64, shared_buffer: &mut [u8]) {
        let last_pos = out_cache.position();
        Self::tbytes_copy_stream_inner(copy_reader, out_cache, shared_buffer, copy_length as usize);
        out_cache.seek(SeekFrom::Start(last_pos)).expect("failed to restore cache position");
    }

    pub(crate) fn tbytes_copy_stream_inner(input: &mut dyn Read, output: &mut Cursor<Vec<u8>>, shared_buffer: &mut [u8], mut read_len: usize) {
        while read_len > 0 {
            let to_read = shared_buffer.len().min(read_len);
            input.read_exact(&mut shared_buffer[..to_read]).expect("failed to read in tbytes_copy_stream_inner");
            output.write_all(&shared_buffer[..to_read]).expect("failed to write in tbytes_copy_stream_inner");
            read_len -= to_read;
        }
    }

    fn tbytes_determine_rle_type(rle_loader: &mut RleRefClip, out_cache: &mut Cursor<Vec<u8>>, mut copy_length: i64, shared_buffer: &mut [u8], mut rle_ctrl_stream: &mut dyn Read, rle_code_stream: &mut dyn Read) {
        Self::tbytes_set_rle(rle_loader, out_cache, &mut copy_length, shared_buffer, rle_code_stream);

        while copy_length > 0 {
            let mut p_sign_buf = [0u8; 1];
            rle_ctrl_stream.read_exact(&mut p_sign_buf).expect("failed to read pSign from rle_ctrl");
            let p_sign = p_sign_buf[0];

            let rle_type = p_sign >> (8 - K_BYTE_RLE_TYPE);
            let mut length = rle_ctrl_stream.read_long_7bit_tagged(K_BYTE_RLE_TYPE, p_sign).expect("failed to read RLE length");
            length += 1;

            if rle_type == 3 {
                rle_loader.mem_copy_length = length;
                Self::tbytes_set_rle(rle_loader, out_cache, &mut copy_length, shared_buffer, rle_code_stream);
                continue;
            }

            rle_loader.mem_set_length = length;
            if rle_type == 2 {
                let mut val = [0u8; 1];
                rle_code_stream.read_exact(&mut val).expect("failed to read RLE set value");
                rle_loader.mem_set_value = val[0];
                Self::tbytes_set_rle(rle_loader, out_cache, &mut copy_length, shared_buffer, rle_code_stream);
                continue;
            }
            rle_loader.mem_set_value = (0u8).wrapping_sub(rle_type);
            Self::tbytes_set_rle(rle_loader, out_cache, &mut copy_length, shared_buffer, rle_code_stream);
        }
    }

    fn tbytes_set_rle(rle_loader: &mut RleRefClip, out_cache: &mut Cursor<Vec<u8>>, copy_length: &mut i64, shared_buffer: &mut [u8], rle_code_stream: &mut dyn Read) {
        Self::tbytes_set_rle_single(rle_loader, out_cache, copy_length, shared_buffer);
        if rle_loader.mem_copy_length == 0 { return; }

        let decode_step = rle_loader.mem_copy_length.min(*copy_length) as usize;
        let last_pos = out_cache.position();
        rle_code_stream.read_exact(&mut shared_buffer[..decode_step]).expect("failed to read from rle_code_stream");
        out_cache.read_exact(&mut shared_buffer[MAX_ARRAY_POOL_SECOND_OFFSET..MAX_ARRAY_POOL_SECOND_OFFSET + decode_step]).expect("failed to read from out_cache");
        out_cache.seek(SeekFrom::Start(last_pos)).expect("failed to restore cache pos");
        Self::tbytes_set_rle_vector_software(rle_loader, out_cache, copy_length, decode_step, shared_buffer, 0, MAX_ARRAY_POOL_SECOND_OFFSET);
    }

    pub(crate) fn tbytes_set_rle_single(rle_loader: &mut RleRefClip, out_cache: &mut Cursor<Vec<u8>>, copy_length: &mut i64, shared_buffer: &mut [u8]) {
        if rle_loader.mem_set_length == 0 { return; }
        let mem_set_step = rle_loader.mem_set_length.min(*copy_length);

        if rle_loader.mem_set_value != 0 {
            let last_pos = out_cache.position();
            let len = mem_set_step as usize;
            out_cache.read_exact(&mut shared_buffer[..len]).expect("failed to read from cache for memset");
            out_cache.seek(SeekFrom::Start(last_pos)).expect("failed to restore cache pos for memset");
            for i in (0..len).rev() { shared_buffer[i] = shared_buffer[i].wrapping_add(rle_loader.mem_set_value); }
            out_cache.write_all(&shared_buffer[..len]).expect("failed to write memset result to cache");
        } else {
            let cur = out_cache.position();
            out_cache.set_position(cur + mem_set_step as u64);
        }
        *copy_length -= mem_set_step;
        rle_loader.mem_set_length -= mem_set_step;
    }

    fn tbytes_set_rle_vector_software(rle_loader: &mut RleRefClip, out_cache: &mut Cursor<Vec<u8>>, copy_length: &mut i64, decode_step: usize, buf: &mut [u8], rle_idx: usize, old_idx: usize) {
        for i in 0..decode_step { buf[rle_idx + i] = buf[rle_idx + i].wrapping_add(buf[old_idx + i]); }
        out_cache.write_all(&buf[rle_idx..rle_idx + decode_step]).expect("failed to write RLE vector result");
        rle_loader.mem_copy_length -= decode_step as i64;
        *copy_length -= decode_step as i64;
    }

    pub fn is_path_a_dir(input: &str) -> bool {
        input.is_empty() || input.ends_with('/')
    }

    fn run_copy_similar_files_routine(&mut self) {
        if let Some(pair) = self.dir_reference_pair.take() {
            self.copy_old_similar_to_new_files(&pair);
            self.dir_reference_pair = Some(pair);
        }
    }

    fn copy_old_similar_to_new_files(&self, dir_data: &DirectoryReferencePair) {
        for pair in &dir_data.data_same_pair_list {
            let new_path = &dir_data.new_utf8_path_list[pair.new_index as usize];
            if Self::is_path_a_dir(new_path) { continue; }
            let old_full = self.path_input.join(&dir_data.old_utf8_path_list[pair.old_index as usize]);
            let new_full = self.path_output.join(new_path);
            if let Some(parent) = new_full.parent() { let _ = std::fs::create_dir_all(parent); }
            let _ = std::fs::copy(&old_full, &new_full);
        }

        let new_ref_count  = dir_data.new_ref_list.len();
        let same_pair_count = dir_data.data_same_pair_list.len();
        let path_count = dir_data.new_utf8_path_list.len();

        let mut cur_new_ref_index  = 0usize;
        let mut cur_path_index     = 0usize;
        let mut cur_same_pair_index = 0usize;

        while cur_path_index < path_count {
            let is_new_ref = cur_new_ref_index < new_ref_count && cur_path_index == dir_data.new_ref_list[cur_new_ref_index] as usize;
            let is_same_pair = cur_same_pair_index < same_pair_count && cur_path_index == dir_data.data_same_pair_list[cur_same_pair_index].new_index as usize;

            if is_new_ref {
                let path = &dir_data.new_utf8_path_list[dir_data.new_ref_list[cur_new_ref_index] as usize];
                if Self::is_path_a_dir(path) { cur_path_index += 1; }
                cur_new_ref_index += 1;
            } else if is_same_pair {
                cur_same_pair_index += 1;
                cur_path_index += 1;
            } else {
                let path = &dir_data.new_utf8_path_list[cur_path_index];
                let combined = self.path_output.join(path);
                if !path.is_empty() {
                    if Self::is_path_a_dir(path) { let _ = std::fs::create_dir_all(&combined); } else if !combined.exists() { let _ = std::fs::File::create(&combined); }
                }
                cur_path_index += 1;
            }
        }
    }
}
