use std::io::{Read, Seek, SeekFrom};
use crate::utils::parser::BinaryExtensions;
use crate::utils::structs::{
    ChecksumMode, DataReferenceInfo, DiffChunkInfo,
    DiffSingleChunkInfo, HeaderInfo,
};

pub(crate) struct Header;

impl Header {
    const HDIFF_HEAD: &'static str = "HDIFF";

    pub fn try_parse_header_info<R: Read + Seek>(sr: &mut R, diff_path: &str, header_info: &mut HeaderInfo, reference_info: &mut DataReferenceInfo) -> Result<bool, Box<dyn std::error::Error>> {
        *header_info = HeaderInfo::default();
        *reference_info = DataReferenceInfo::default();

        let header_info_line = sr.read_string_to_null(512)?;
        let mut is_patch_dir = true;

        #[cfg(debug_assertions)]
        println!("[Header::TryParseHeaderInfo] Signature info: {}", header_info_line);

        if header_info_line.len() > 64 || !header_info_line.starts_with(Self::HDIFF_HEAD) { return Err("[Header::TryParseHeaderInfo] This is not a HDiff file format!".into()); }
        let h_info_arr: Vec<&str> = header_info_line.split('&').collect();

        if h_info_arr.len() == 2 {
            // Single-file patch format: "HDIFF13&zstd"
            let p_file_ver = Self::try_get_version(h_info_arr[0])?;
            if p_file_ver != 13 { return Err("[Header::TryParseHeaderInfo] HDiff version is unsupported. This patcher only supports the single patch file with version: 13 only!".into()); }
            is_patch_dir = false;
            header_info.header_magic = h_info_arr[0].to_string();
            header_info.comp_mode = h_info_arr[1].parse().map_err(|e: String| e)?;

            #[cfg(debug_assertions)]
            println!("[Header::TryParseHeaderInfo] Version: {} Compression: {:?}", p_file_ver, header_info.comp_mode);
        } else if h_info_arr.len() != 3 { return Err(format!("[Header::TryParseHeaderInfo] Header info is incomplete! Expecting 3 parts but got {} part(s) instead (Raw: {})", h_info_arr.len(), header_info_line).into()); }

        if is_patch_dir {
            // Directory patch format: "HDIFF19&zstd&fadler64"
            let h_info_ver = Self::try_get_version(h_info_arr[0])?;
            if h_info_ver != 19 { return Err("[Header::TryParseHeaderInfo] HDiff version is unsupported. This patcher only supports the directory patch file with version: 19 only!".into()); }

            if !h_info_arr[1].is_empty() {
                header_info.comp_mode = h_info_arr[1].parse().map_err(|_: String| { format!("[Header::TryParseHeaderInfo] This patcher doesn't support {} compression!", h_info_arr[1]) })?;
            }

            if h_info_arr[2].is_empty() {
                header_info.checksum_mode = ChecksumMode::Nochecksum;
            } else {
                header_info.checksum_mode = h_info_arr[2].parse().map_err(|_: String| { format!("[Header::TryParseHeaderInfo] This patcher doesn't support {} checksum!", h_info_arr[2]) })?;
            }

            #[cfg(debug_assertions)]
            println!("[Header::TryParseHeaderInfo] Version: {} ChecksumMode: {:?} Compression: {:?}", h_info_ver, header_info.checksum_mode, header_info.comp_mode);

            Self::try_read_header_and_reference_info(sr, header_info, reference_info)?;
            Self::try_read_extern_reference_info(sr, diff_path, header_info, reference_info)?;
        } else {
            Self::try_read_non_single_file_header_info(sr, diff_path, header_info)?;
        }
        Ok(is_patch_dir)
    }

    fn try_read_extern_reference_info<R: Read + Seek>(sr: &mut R, diff_path: &str, header_info: &mut HeaderInfo, reference_info: &mut DataReferenceInfo) -> Result<(), Box<dyn std::error::Error>> {
        let cur_pos = sr.stream_position()? as i64;
        reference_info.head_data_offset = cur_pos;

        let cur_pos = cur_pos + if reference_info.head_data_compressed_size > 0 { reference_info.head_data_compressed_size } else { reference_info.head_data_size };
        reference_info.private_extern_data_offset = cur_pos;

        let cur_pos = cur_pos + reference_info.private_extern_data_size;
        reference_info.extern_data_offset = cur_pos;

        let cur_pos = cur_pos + reference_info.extern_data_size;
        reference_info.hdiff_data_offset = cur_pos;

        let total_len = {
            let saved = sr.stream_position()?;
            let end = sr.seek(SeekFrom::End(0))?;
            sr.seek(SeekFrom::Start(saved))?;
            end as i64
        };
        reference_info.hdiff_data_size = total_len - cur_pos;

        #[cfg(debug_assertions)]
        println!("[Header::TryReadExternReferenceInfo] headDataOffset: {} | privateExternDataOffset: {} | externDataOffset: {} | hdiffDataOffset: {} | hdiffDataSize: {}", reference_info.head_data_offset, reference_info.private_extern_data_offset, reference_info.extern_data_offset, reference_info.hdiff_data_offset, reference_info.hdiff_data_size);

        Self::try_identify_diff_type(sr, diff_path, header_info, reference_info)
    }

    fn try_identify_diff_type<R: Read + Seek>(sr: &mut R, diff_path: &str, header_info: &mut HeaderInfo, reference_info: &mut DataReferenceInfo) -> Result<(), Box<dyn std::error::Error>> {
        sr.seek(SeekFrom::Start(reference_info.hdiff_data_offset as u64))?;
        let single_compressed_header_line = sr.read_string_to_null(512)?;
        let single_compressed_header_arr: Vec<&str> = single_compressed_header_line.split('&').collect();

        header_info.is_single_compressed_diff = single_compressed_header_arr[0] == "HDIFFSF20";
        if header_info.is_single_compressed_diff {
            Self::try_read_single_file_header_info(sr, diff_path, header_info, reference_info)?;
            return Ok(());
        }

        #[cfg(debug_assertions)]
        println!("[Header::TryIdentifyDiffType] HDIFF Dir Signature: {}", single_compressed_header_line);

        if single_compressed_header_arr.len() > 1 && !single_compressed_header_arr[1].is_empty() {
            header_info.comp_mode = single_compressed_header_arr[1].parse().map_err(|_: String| { format!("[Header::TryIdentifyDiffType] Unsupported compression: {}", single_compressed_header_arr[1]) })?;
        }
        header_info.header_magic = single_compressed_header_arr[0].to_string();

        Self::try_read_non_single_file_header_info(sr, diff_path, header_info)
    }

    fn try_read_single_file_header_info<R: Read + Seek>(sr: &mut R, diff_path: &str, header_info: &mut HeaderInfo, reference_info: &DataReferenceInfo) -> Result<(), Box<dyn std::error::Error>> {
        header_info.patch_path = diff_path.to_string();
        header_info.single_chunk_info = DiffSingleChunkInfo::default();

        header_info.new_data_size = sr.read_long_7bit()?;
        header_info.old_data_size = sr.read_long_7bit()?;

        #[cfg(debug_assertions)]
        println!("[Header::TryReadSingleFileHeaderInfo] oldDataSize: {} | newDataSize: {}", header_info.old_data_size, header_info.new_data_size);

        header_info.chunk_info.cover_count = sr.read_long_7bit()?;
        header_info.step_mem_size = sr.read_long_7bit()?;
        header_info.single_chunk_info.uncompressed_size = sr.read_long_7bit()?;
        header_info.single_chunk_info.compressed_size = sr.read_long_7bit()?;

        let pos = sr.stream_position()? as i64;
        header_info.single_chunk_info.diff_data_pos = pos - reference_info.hdiff_data_offset;
        header_info.compressed_count = if header_info.single_chunk_info.compressed_size > 0 { 1 } else { 0 };

        #[cfg(debug_assertions)]
        println!("[Header::TryReadSingleFileHeaderInfo] compressedCount: {}", header_info.compressed_count);

        Ok(())
    }

    fn try_read_non_single_file_header_info<R: Read + Seek>(sr: &mut R, diff_path: &str, header_info: &mut HeaderInfo) -> Result<(), Box<dyn std::error::Error>> {
        header_info.patch_path = diff_path.to_string();

        let type_end_pos = sr.stream_position()? as i64;
        header_info.new_data_size = sr.read_long_7bit()?;
        header_info.old_data_size = sr.read_long_7bit()?;

        #[cfg(debug_assertions)]
        println!("[Header::TryReadNonSingleFileHeaderInfo] oldDataSize: {} | newDataSize: {}", header_info.old_data_size, header_info.new_data_size);

        Self::get_diff_chunk_info(sr, &mut header_info.chunk_info, type_end_pos)?;
        header_info.compressed_count = ((header_info.chunk_info.compress_cover_buf_size > 1) as i64) + ((header_info.chunk_info.compress_rle_ctrl_buf_size > 1) as i64) + ((header_info.chunk_info.compress_rle_code_buf_size > 1) as i64) + ((header_info.chunk_info.compress_new_data_diff_size > 1) as i64);

        #[cfg(debug_assertions)]
        println!("[Header::TryReadNonSingleFileHeaderInfo] compressedCount: {}", header_info.compressed_count);

        Ok(())
    }

    fn get_diff_chunk_info<R: Read + Seek>(sr: &mut R, chunk_info: &mut DiffChunkInfo, type_end_pos: i64) -> Result<(), Box<dyn std::error::Error>> {
        *chunk_info = DiffChunkInfo::default();
        chunk_info.types_end_pos = type_end_pos;

        #[cfg(debug_assertions)]
        println!("[Header::GetDiffChunkInfo] typesEndPos: {}", type_end_pos);

        chunk_info.cover_count = sr.read_long_7bit()?;
        chunk_info.compress_size_begin_pos = sr.stream_position()? as i64;

        #[cfg(debug_assertions)]
        println!("[Header::GetDiffChunkInfo] coverCount: {} | compressSizeBeginPos: {}", chunk_info.cover_count, chunk_info.compress_size_begin_pos);

        chunk_info.cover_buf_size              = sr.read_long_7bit()?;
        chunk_info.compress_cover_buf_size     = sr.read_long_7bit()?;
        chunk_info.rle_ctrl_buf_size           = sr.read_long_7bit()?;
        chunk_info.compress_rle_ctrl_buf_size  = sr.read_long_7bit()?;
        chunk_info.rle_code_buf_size           = sr.read_long_7bit()?;
        chunk_info.compress_rle_code_buf_size  = sr.read_long_7bit()?;
        chunk_info.new_data_diff_size          = sr.read_long_7bit()?;
        chunk_info.compress_new_data_diff_size = sr.read_long_7bit()?;

        #[cfg(debug_assertions)]
        {
            println!("[Header::GetDiffChunkInfo] cover_buf_size: {} | compress_cover_buf_size: {}", chunk_info.cover_buf_size, chunk_info.compress_cover_buf_size);
            println!("[Header::GetDiffChunkInfo] rle_ctrlBuf_size: {} | compress_rle_ctrlBuf_size: {}", chunk_info.rle_ctrl_buf_size, chunk_info.compress_rle_ctrl_buf_size);
            println!("[Header::GetDiffChunkInfo] rle_codeBuf_size: {} | compress_rle_codeBuf_size: {}", chunk_info.rle_code_buf_size, chunk_info.compress_rle_code_buf_size);
            println!("[Header::GetDiffChunkInfo] newDataDiff_size: {} | compress_newDataDiff_size: {}", chunk_info.new_data_diff_size, chunk_info.compress_new_data_diff_size);
        }

        chunk_info.head_end_pos = sr.stream_position()? as i64;
        chunk_info.cover_end_pos = chunk_info.head_end_pos + if chunk_info.compress_cover_buf_size > 0 { chunk_info.compress_cover_buf_size } else { chunk_info.cover_buf_size };

        #[cfg(debug_assertions)]
        println!("[Header::GetDiffChunkInfo] headEndPos: {} | coverEndPos: {}", chunk_info.head_end_pos, chunk_info.cover_end_pos);

        Ok(())
    }

    fn try_read_header_and_reference_info<R: Read + Seek>(sr: &mut R, header_info: &mut HeaderInfo, reference_info: &mut DataReferenceInfo) -> Result<(), Box<dyn std::error::Error>> {
        header_info.is_input_dir  = sr.read_boolean()?;
        header_info.is_output_dir = sr.read_boolean()?;

        #[cfg(debug_assertions)]
        println!("[Header::TryReadHeaderAndReferenceInfo] Is In/Out a Dir -> Input: {} / Output: {}", header_info.is_input_dir, header_info.is_output_dir);

        reference_info.input_dir_count  = sr.read_long_7bit()?;
        reference_info.input_sum_size   = sr.read_long_7bit()?;
        reference_info.output_dir_count = sr.read_long_7bit()?;
        reference_info.output_sum_size  = sr.read_long_7bit()?;

        #[cfg(debug_assertions)]
        println!("[Header::TryReadHeaderAndReferenceInfo] InDir Count/SumSize: {}/{} | OutDir Count/SumSize: {}/{}", reference_info.input_dir_count, reference_info.input_sum_size, reference_info.output_dir_count, reference_info.output_sum_size);

        reference_info.input_ref_file_count  = sr.read_long_7bit()?;
        reference_info.input_ref_file_size   = sr.read_long_7bit()?;
        reference_info.output_ref_file_count = sr.read_long_7bit()?;
        reference_info.output_ref_file_size  = sr.read_long_7bit()?;

        #[cfg(debug_assertions)]
        println!("[Header::TryReadHeaderAndReferenceInfo] InRef Count/Size: {}/{} | OutRef Count/Size: {}/{}", reference_info.input_ref_file_count, reference_info.input_ref_file_size, reference_info.output_ref_file_count, reference_info.output_ref_file_size);

        reference_info.same_file_pair_count = sr.read_long_7bit()?;
        reference_info.same_file_size       = sr.read_long_7bit()?;

        #[cfg(debug_assertions)]
        println!("[Header::TryReadHeaderAndReferenceInfo] IdenticalPair Count/Size: {}/{}", reference_info.same_file_pair_count, reference_info.same_file_size);

        reference_info.new_execute_count          = sr.read_int_7bit()?;
        reference_info.private_reserved_data_size = sr.read_long_7bit()?;
        reference_info.private_extern_data_size   = sr.read_long_7bit()?;
        reference_info.extern_data_size           = sr.read_long_7bit()?;

        #[cfg(debug_assertions)]
        println!("[Header::TryReadHeaderAndReferenceInfo] newExecuteCount: {} | privateReservedDataSize: {} | privateExternDataSize: {} | externDataSize: {}", reference_info.new_execute_count, reference_info.private_reserved_data_size, reference_info.private_extern_data_size, reference_info.extern_data_size);

        reference_info.compress_size_begin_pos = sr.stream_position()? as i64;

        reference_info.head_data_size           = sr.read_long_7bit()?;
        reference_info.head_data_compressed_size = sr.read_long_7bit()?;
        reference_info.checksum_byte_size       = sr.read_long_7bit()? as u8;
        header_info.dir_data_is_compressed      = reference_info.head_data_compressed_size > 0;
        reference_info.checksum_offset          = sr.stream_position()? as i64;

        #[cfg(debug_assertions)]
        {
            println!("[Header::TryReadHeaderAndReferenceInfo] compressSizeBeginPos: {} | headDataSize: {} |  headDataCompressedSize: {} | checksumByteSize: {}", reference_info.compress_size_begin_pos, reference_info.head_data_size, reference_info.head_data_compressed_size, reference_info.checksum_byte_size);
            println!("[Header::TryReadHeaderAndReferenceInfo] checksumOffset: {} | dirDataIsCompressed: {}", reference_info.checksum_offset, header_info.dir_data_is_compressed);
        }

        if reference_info.checksum_byte_size > 0 {
            let skip = (reference_info.checksum_byte_size as i32) * 4;
            #[cfg(debug_assertions)]
            println!("[Header::TryReadHeaderAndReferenceInfo] Seeking += {} bytes from checksum bytes!", skip);

            Self::try_seek_header(sr, skip)?;
        }

        Ok(())
    }

    fn try_seek_header<R: Read + Seek>(sr: &mut R, skip_long_size: i32) -> Result<(), Box<dyn std::error::Error>> {
        let len = skip_long_size.min(4 << 10);
        sr.seek(SeekFrom::Current(len as i64))?;
        Ok(())
    }

    fn try_get_version(str_val: &str) -> Result<i64, Box<dyn std::error::Error>> {
        let idx = str_val.find(Self::HDIFF_HEAD).ok_or_else(|| { format!("[Header::TryGetVersion] Cannot find 'HDIFF' in: {}", str_val) })?;
        let num_str = &str_val[idx + Self::HDIFF_HEAD.len()..];
        num_str.parse::<i64>().map_err(|_| { format!("[Header::TryGetVersion] Invalid version string: {} (Raw: {})", num_str, str_val).into() })
    }
}
