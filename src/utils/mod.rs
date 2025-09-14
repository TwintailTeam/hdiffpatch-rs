use std::{fs, io};
use std::io::{Cursor, Read, Seek};
use std::path::Path;
use crate::utils::parser::{Parser};

pub(crate) mod parser;

// === KRDIFF STRUCTS ===

// We do not talk why hdiffpatch has this cursed thing btw
#[derive(Clone, Debug)]
pub struct VarInt(pub(crate) i64);

#[derive(Debug, Clone)]
pub struct Cover {
    pub old_pos: i64,
    pub new_pos: u64,
    pub length: u64,
}

#[derive(Debug, Clone)]
pub struct CoverBuf {
    pub covers: Vec<Cover>,
}

impl CoverBuf {
    pub fn parse(parser: &mut Parser<impl Read + Seek>, size: u64, compressed_size: u64, cover_count: u64) -> Self {
        let data = parser.read_maybe_compressed(size, compressed_size);
        let mut sub_parser = Parser::from_reader(Cursor::new(&data), "cover_buf");
        assert_eq!(data.len() as u64, size);

        let mut covers = Vec::with_capacity(cover_count as usize);
        for _ in 0..cover_count {
            let old_pos = sub_parser.read_varint(1).0;
            let new_pos = sub_parser.read_varint(0).0 as u64;
            let length = sub_parser.read_varint(0).0 as u64;
            covers.push(Cover { old_pos, new_pos, length, });
        }
        sub_parser.check_read(size);
        CoverBuf { covers }
    }
}

#[derive(Debug, Clone)]
pub struct DiffZ {
    pub compress_type: String,
    pub new_data_size: VarInt,
    pub old_data_size: VarInt,
    pub cover_count: VarInt,
    pub cover_buf_size: VarInt,
    pub compressed_cover_buf_size: VarInt,
    pub rle_ctrl_buf_size: VarInt,
    pub compressed_rle_ctrl_buf_size: VarInt,
    pub rle_code_buf_size: VarInt,
    pub compressed_rle_code_buf_size: VarInt,
    pub new_data_diff_size: VarInt,
    pub compressed_new_data_diff_size: VarInt,
    pub cover_buf: CoverBuf,
    pub new_data_offset: u64,
}

impl DiffZ {
    pub fn parse(parser: &mut Parser<impl Read + Seek>) -> Self {
        let expected_header = b"HDIFF13&zstd\0";
        let compress_type = "zstd".to_string();
        parser.match_bytes(expected_header);

        let new_data_size = parser.read_varint(0);
        let old_data_size = parser.read_varint(0);
        let cover_count = parser.read_varint(0);
        let cover_buf_size = parser.read_varint(0);
        let compressed_cover_buf_size = parser.read_varint(0);
        let rle_ctrl_buf_size = parser.read_varint(0);
        let compressed_rle_ctrl_buf_size = parser.read_varint(0);
        let rle_code_buf_size = parser.read_varint(0);
        let compressed_rle_code_buf_size = parser.read_varint(0);
        let new_data_diff_size = parser.read_varint(0);
        let compressed_new_data_diff_size = parser.read_varint(0);

        let cover_buf = CoverBuf::parse(parser, cover_buf_size.0 as u64, compressed_cover_buf_size.0 as u64, cover_count.0 as u64);
        DiffZ { compress_type, new_data_size, old_data_size, cover_count, cover_buf_size, compressed_cover_buf_size, rle_ctrl_buf_size, compressed_rle_ctrl_buf_size, rle_code_buf_size, compressed_rle_code_buf_size, new_data_diff_size, compressed_new_data_diff_size, cover_buf, new_data_offset: 0 }
    }
}

#[derive(Debug, Clone)]
pub struct File {
    pub name: String,
    pub file_offset: u8,
    pub file_size: u64,
}

#[derive(Debug, Clone)]
pub struct Directory {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct HeadData {
    pub old_files: Vec<File>,
    pub new_files: Vec<File>,
    pub old_dirs: Vec<Directory>,
    pub new_dirs: Vec<Directory>,
}

impl HeadData {
    pub fn parse(parser: &mut Parser<impl Read + Seek>, size: u64, compressed_size: u64, old_path_count: u64, new_path_count: u64, old_ref_file_count: u64, new_ref_file_count: u64, ) -> Self {
        let data = parser.read_maybe_compressed(size, compressed_size);
        let mut sub_parser = parser.sub_parser(&data, "head_data");

        let mut old_files_names = Vec::with_capacity(old_path_count as usize);
        let mut new_files_names = Vec::with_capacity(new_path_count as usize);
        let mut old_file_offsets = Vec::with_capacity(old_ref_file_count as usize);
        let mut new_file_offsets = Vec::with_capacity(new_ref_file_count as usize);
        let mut old_file_sizes = Vec::with_capacity(old_ref_file_count as usize);
        let mut new_file_sizes = Vec::with_capacity(new_ref_file_count as usize);
        let mut unknown = Vec::with_capacity(new_ref_file_count as usize);

        // Read old file names
        for _ in 0..old_path_count { old_files_names.push(sub_parser.read_string()); }
        // Read new file names
        for _ in 0..new_path_count { new_files_names.push(sub_parser.read_string()); }
        // Read old file offsets
        for _ in 0..old_ref_file_count {
            let v = sub_parser.read_varint(0);
            assert!(v.0 < 128);
            old_file_offsets.push(v.0 as u8);
        }
        // Read new file offsets
        for _ in 0..new_ref_file_count {
            let v = sub_parser.read_varint(0);
            assert!(v.0 < 128);
            new_file_offsets.push(v.0 as u8);
        }
        // Read old file sizes
        for _ in 0..old_ref_file_count { old_file_sizes.push(sub_parser.read_varint(0)); }
        // Read new file sizes
        for _ in 0..new_ref_file_count { new_file_sizes.push(sub_parser.read_varint(0)); }
        // Read unknown varints
        for _ in 0..new_ref_file_count { unknown.push(sub_parser.read_varint(0)); }

        let mut head = HeadData { old_files: Vec::with_capacity(old_ref_file_count as usize), new_files: Vec::with_capacity(new_ref_file_count as usize), old_dirs: Vec::with_capacity((old_path_count - old_ref_file_count) as usize), new_dirs: Vec::with_capacity((new_path_count - new_ref_file_count) as usize) };
        // Build old files and directories
        let mut j = 0;
        for i in 0..old_path_count as usize {
            let name = &old_files_names[i];
            if name.ends_with('/') || name.is_empty() {
                head.old_dirs.push(Directory { name: name.clone() });
            } else {
                head.old_files.push(File { name: name.clone(), file_offset: old_file_offsets[j], file_size: old_file_sizes[j].0 as u64 });
                j += 1;
            }
        }
        // Build new files and directories
        j = 0;
        for i in 0..new_path_count as usize {
            let name = &new_files_names[i];
            if name.ends_with('/') || name.is_empty() {
                head.new_dirs.push(Directory { name: name.clone() });
            } else {
                head.new_files.push(File { name: name.clone(), file_offset: new_file_offsets[j], file_size: new_file_sizes[j].0 as u64 });
                j += 1;
            }
        }
        sub_parser.check_read(size);
        assert_eq!(head.old_files.len(), old_ref_file_count as usize);
        assert_eq!(head.new_files.len(), new_ref_file_count as usize);
        assert_eq!(head.old_dirs.len(), (old_path_count - old_ref_file_count) as usize);
        assert_eq!(head.new_dirs.len(), (new_path_count - new_ref_file_count) as usize);
        head
    }
}

#[derive(Debug, Clone)]
pub struct DirDiff {
    pub compression_type: String,
    pub checksum_type: String,
    pub old_path_is_dir: bool,
    pub new_path_is_dir: bool,
    pub old_path_count: VarInt,
    pub new_path_count: VarInt,
    pub old_path_sum_size: VarInt,
    pub new_path_sum_size: VarInt,
    pub old_ref_file_count: VarInt,
    pub new_ref_file_count: VarInt,
    pub old_ref_size: VarInt,
    // Always equal to 0
    // pub same_file_pair_count: VarInt,
    // pub same_file_size: VarInt,
    // pub new_execute_count: VarInt,
    // pub private_reserved_data_size: VarInt,
    // pub private_extern_data_size: VarInt,
    // pub extern_data_size: VarInt,
    pub new_ref_size: VarInt,
    pub head_data_size: VarInt,
    pub head_data_compressed_size: VarInt,
    pub checksum_byte_size: VarInt,
    pub checksum: Vec<u8>,
    pub head_data: HeadData,
    pub main_diff: DiffZ,
}

impl DirDiff {
    pub fn parse(parser: &mut Parser<impl Read + Seek>) -> Self {
        // HDIFF19 & compressionType & checksumType \0 oldPathIsDir newPathIsDir
        let expected_header = b"HDIFF19&zstd&fadler64\0\x01\x01";
        parser.match_bytes(expected_header);

        let compression_type = "zstd".to_string();
        let checksum_type = "fadler64".to_string();
        let old_path_is_dir = true;
        let new_path_is_dir = true;

        let old_path_count = parser.read_varint(0);
        let old_path_sum_size = parser.read_varint(0);
        let new_path_count = parser.read_varint(0);
        let new_path_sum_size = parser.read_varint(0);
        let old_ref_file_count = parser.read_varint(0);
        let old_ref_size = parser.read_varint(0);
        let new_ref_file_count = parser.read_varint(0);
        let new_ref_size = parser.read_varint(0);

        // The following varints are always zero
        parser.match_varint(0); // sameFilePairCount
        parser.match_varint(0); // sameFileSize
        parser.match_varint(0); // newExecuteCount
        parser.match_varint(0); // privateReservedDataSize
        parser.match_varint(0); // privateExternDataSize
        parser.match_varint(0); // externDataSize

        let head_data_size = parser.read_varint(0);
        let head_data_compressed_size = parser.read_varint(0);
        let checksum_byte_size = parser.read_varint(0);
        let checksum_len = (checksum_byte_size.0 as usize) * 4;
        let checksum = parser.read_bytes::<u8>(checksum_len);

        let head_data = HeadData::parse(parser, head_data_size.0 as u64, head_data_compressed_size.0 as u64, old_path_count.0 as u64, new_path_count.0 as u64, old_ref_file_count.0 as u64, new_ref_file_count.0 as u64);
        let main_diff = DiffZ::parse(parser);
        Self { compression_type, checksum_type, old_path_is_dir, new_path_is_dir, old_path_count, old_path_sum_size, new_path_count, new_path_sum_size, old_ref_file_count, old_ref_size, new_ref_file_count, new_ref_size, head_data_size, head_data_compressed_size, checksum_byte_size, checksum, head_data, main_diff }
    }
}

pub fn merge_dirs_recursive(a: &Path, b: &Path) -> io::Result<()> {
    for entry in fs::read_dir(b)? {
        let entry = entry?;
        let path = entry.path();
        let relative_path = path.strip_prefix(b).unwrap();
        let target_path = a.join(relative_path);

        if path.is_dir() { fs::create_dir_all(&target_path)?; merge_dirs_recursive(a, &path)?;
        } else {
            if let Some(parent) = target_path.parent() { fs::create_dir_all(parent)?; }
            if let Err(e) = fs::rename(&path, &target_path) { panic!("Error copying {} to {}: {}", path.display(), target_path.display(), e); }
        }
    }
    Ok(())
}