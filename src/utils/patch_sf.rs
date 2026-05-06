use std::fs::File;
use std::io::{Cursor, Read, SeekFrom, Write};
use crate::utils::compression_utils::get_clip_stream;
use crate::utils::parser::BinaryExtensions;
use crate::utils::structs::{HeaderInfo, SeekableRead};

pub struct PatchSF {
    header_info: HeaderInfo,
}

impl PatchSF {
    pub fn new(header_info: HeaderInfo) -> Self {
        Self { header_info }
    }

    pub fn patch(&self, input_stream: &mut dyn SeekableRead, output_stream: &mut dyn Write, patch_path: &str, _write_bytes_cb: Option<Box<dyn FnMut(i64)>>) -> std::io::Result<()> {
        let sci = &self.header_info.single_chunk_info;
        let (mut diff, _) = get_clip_stream(File::open(patch_path)?, self.header_info.comp_mode, sci.diff_data_pos as u64, sci.uncompressed_size as u64, sci.compressed_size as u64, false)?;
        self.start_patch_routine(&mut diff, input_stream, output_stream)
    }

    fn start_patch_routine(&self, diff: &mut dyn Read, old: &mut dyn SeekableRead, out: &mut dyn Write) -> std::io::Result<()> {
        let cover_count = self.header_info.chunk_info.cover_count as u64;
        let step_mem_size = self.header_info.step_mem_size as usize;
        let mut step_buf = vec![0u8; step_mem_size];
        let mut io_buf = vec![0u8; step_mem_size];
        patch_loop(diff, old, out, cover_count, &mut step_buf, &mut io_buf)
    }
}

fn patch_loop(mut diff: &mut dyn Read, old: &mut dyn SeekableRead, out: &mut dyn Write, mut cover_count: u64, step_buf: &mut Vec<u8>, io_buf: &mut Vec<u8>) -> std::io::Result<()> {
    let mut last_old_end = 0u64;
    let mut last_new_end = 0u64;

    while cover_count > 0 {
        let buf_cover_size = diff.read_long_7bit()? as usize;
        let buf_rle_size = diff.read_long_7bit()? as usize;
        let step_end = buf_cover_size + buf_rle_size;

        if step_end > step_buf.len() { step_buf.resize(step_end, 0); }
        diff.read_exact(&mut step_buf[..step_end])?;

        let (covers_slice, rle_slice) = step_buf[..step_end].split_at(buf_cover_size);
        let mut covers = Cursor::new(covers_slice);
        let covers_len = covers_slice.len() as u64;
        let mut rle0 = Rle0Decoder::new(rle_slice);

        while covers.position() < covers_len && cover_count > 0 {
            let prev_new_end = last_new_end;
            let (old_pos, new_pos, length) = decode_cover(&mut covers, &mut last_old_end, &mut last_new_end)?;
            if new_pos > prev_new_end { copy_n(&mut *diff, out, new_pos - prev_new_end, io_buf)?; }
            cover_count -= 1;

            if length > 0 {
                old.seek(SeekFrom::Start(old_pos))?;
                let mut rem = length;
                while rem > 0 {
                    let take = (io_buf.len() as u64).min(rem) as usize;
                    old.read_exact(&mut io_buf[..take])?;
                    rle0.add(&mut io_buf[..take]);
                    out.write_all(&io_buf[..take])?;
                    rem -= take as u64;
                }
            }
        }
    }
    Ok(())
}

fn decode_cover(covers: &mut Cursor<&[u8]>, last_old_end: &mut u64, last_new_end: &mut u64) -> std::io::Result<(u64, u64, u64)> {
    let mut b = [0u8; 1];
    covers.read_exact(&mut b)?;
    let first = b[0];
    let sign = first >> 7;
    let delta = covers.read_long_7bit_tagged(1, first)? as u64;
    let old_pos = if sign == 0 { last_old_end.wrapping_add(delta) } else { last_old_end.wrapping_sub(delta) };
    let new_pos = last_new_end.wrapping_add(covers.read_long_7bit()? as u64);
    let length = covers.read_long_7bit()? as u64;
    *last_old_end = old_pos.wrapping_add(length);
    *last_new_end = new_pos.wrapping_add(length);
    Ok((old_pos, new_pos, length))
}

struct Rle0Decoder<'a> {
    buf: &'a [u8],
    pos: usize,
    len0: usize,
    lenv: usize,
    need_decode0: bool,
}

impl<'a> Rle0Decoder<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0, len0: 0, lenv: 0, need_decode0: true }
    }

    fn add(&mut self, data: &mut [u8]) {
        let mut dp = 0usize;
        let mut rem = data.len();
        while rem > 0 {
            if self.len0 > 0 {
                let take = self.len0.min(rem);
                self.len0 -= take; dp += take; rem -= take;
            } else if self.lenv > 0 {
                let take = self.lenv.min(rem);
                let src = &self.buf[self.pos..self.pos + take];
                for i in 0..take { data[dp + i] = data[dp + i].wrapping_add(src[i]); }
                self.pos += take; self.lenv -= take; dp += take; rem -= take;
            } else if self.need_decode0 {
                self.need_decode0 = false;
                self.len0 = rle_varint(self.buf, &mut self.pos);
            } else {
                self.need_decode0 = true;
                self.lenv = rle_varint(self.buf, &mut self.pos);
            }
        }
    }
}

fn rle_varint(buf: &[u8], pos: &mut usize) -> usize {
    let first = buf[*pos]; *pos += 1;
    let mut val = (first & 0x7F) as u64;
    if (first & 0x80) != 0 {
        loop {
            let b = buf[*pos]; *pos += 1;
            val = (val << 7) | (b & 0x7F) as u64;
            if (b & 0x80) == 0 { break; }
        }
    }
    val as usize
}

fn copy_n(src: &mut dyn Read, dst: &mut dyn Write, mut n: u64, buf: &mut [u8]) -> std::io::Result<()> {
    while n > 0 {
        let take = (buf.len() as u64).min(n) as usize;
        src.read_exact(&mut buf[..take])?;
        dst.write_all(&buf[..take])?;
        n -= take as u64;
    }
    Ok(())
}
