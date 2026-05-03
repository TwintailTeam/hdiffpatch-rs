use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use crate::utils::structs::{CombinedStream, NewFileCombinedStream, PairIndexReference};

impl<T: Read> BinaryExtensions for T {}

pub(crate) fn get_file_stream_buffer_size(file_size: u64) -> usize {
    match file_size {
        0..=131_072 => 4 * 1024,
        131_073..=1_048_576 => 64 * 1024,
        1_048_577..=33_554_432 => 128 * 1024,
        33_554_433..=104_857_600 => 512 * 1024,
        _ => 1 * 1024 * 1024,
    }
}

pub(crate) trait BinaryExtensions: Read {
    fn read_boolean(&mut self) -> std::io::Result<bool> {
        let mut b = [0u8; 1];
        self.read_exact(&mut b)?;
        Ok(b[0] != 0)
    }

    fn read_string_to_null(&mut self, _buffer_size: usize) -> std::io::Result<String> {
        let mut buf = Vec::with_capacity(64);
        let mut byte = [0u8; 1];
        loop {
            let n = self.read(&mut byte)?;
            if n == 0 || byte[0] == 0 { break; }
            buf.push(byte[0]);
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn read_int_7bit(&mut self) -> std::io::Result<i32> {
        self.read_int_7bit_tagged(0, 0)
    }

    fn read_int_7bit_tagged(&mut self, tag_bit: u8, prev_byte: u8) -> std::io::Result<i32> {
        let code = if tag_bit != 0 {
            prev_byte
        } else {
            let mut b = [0u8; 1];
            self.read_exact(&mut b)?;
            b[0]
        };
        let mask = (1u8 << (7 - tag_bit)).wrapping_sub(1);
        let mut value = (code & mask) as i32;

        if (code & (1 << (7 - tag_bit))) == 0 { return Ok(value); }
        loop {
            if (value >> (4 * 4 - 7)) != 0 { return Ok(0); }
            let mut b = [0u8; 1];
            self.read_exact(&mut b)?;
            let code = b[0];
            value = (value << 7) | ((code & 0x7F) as i32);
            if (code & 0x80) == 0 { break; }
        }
        Ok(value)
    }

    fn read_long_7bit(&mut self) -> std::io::Result<i64> {
        self.read_long_7bit_tagged(0, 0)
    }

    fn read_long_7bit_tagged(&mut self, tag_bit: u8, prev_byte: u8) -> std::io::Result<i64> {
        let code = if tag_bit != 0 {
            prev_byte
        } else {
            let mut b = [0u8; 1];
            self.read_exact(&mut b)?;
            b[0]
        };
        let mask = (1u8 << (7 - tag_bit)).wrapping_sub(1);
        let mut value = (code & mask) as i64;
        if (code & (1 << (7 - tag_bit))) == 0 { return Ok(value); }
        loop {
            if (value >> (8 * 8 - 7)) != 0 { return Ok(0); }
            let mut b = [0u8; 1];
            self.read_exact(&mut b)?;
            let code = b[0];
            value = (value << 7) | ((code & 0x7F) as i64);
            if (code & 0x80) == 0 { break; }
        }
        Ok(value)
    }

    fn get_longs_from_stream(&mut self, count: usize, check_count: Option<i64>) -> std::io::Result<Vec<i64>> {
        let mut out = Vec::with_capacity(count);
        let mut back_value = -1i64;

        for i in 0..count {
            let num = self.read_long_7bit()?;
            back_value += 1 + num;
            if let Some(max_val) = check_count {
                if back_value > max_val { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("[get_longs_from_stream] Invalid back value at {}, expected max {}", i, max_val))); }
            }
            out.push(back_value);
        }
        Ok(out)
    }

    fn get_longs_from_stream_absolute(&mut self, count: usize) -> std::io::Result<Vec<i64>> {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count { out.push(self.read_long_7bit()?); }
        Ok(out)
    }

    fn get_pair_index_reference_from_stream(&mut self, pair_count: usize, check_end_new: i64, check_end_old: i64) -> std::io::Result<Vec<PairIndexReference>> {
        let mut result = Vec::with_capacity(pair_count);
        let mut back_new = -1i64;
        let mut back_old = -1i64;

        for i in 0..pair_count {
            let inc_new = self.read_long_7bit()?;
            back_new += 1 + inc_new;
            if back_new > check_end_new { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Invalid new index at {} with value {}", i, back_new))); }

            let mut sign = [0u8; 1];
            self.read_exact(&mut sign)?;
            let p_sign = sign[0];
            let inc_old = self.read_long_7bit_tagged(1, p_sign)?;

            if (p_sign >> 7) == 0 { back_old += 1 + inc_old; } else { back_old = back_old + 1 - inc_old; }
            if back_old > check_end_old { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Invalid old index at {} with value {}", i, back_old))); }
            result.push(PairIndexReference { new_index: back_new, old_index: back_old });
        }
        Ok(result)
    }

    fn get_paths_from_stream(&mut self, buf_size: usize, count: usize) -> std::io::Result<Vec<String>> {
        let mut buffer = vec![0u8; buf_size];
        self.read_exact(&mut buffer)?;

        let mut paths = Vec::with_capacity(count);
        let mut cur_start = 0usize;
        for (i, &b) in buffer.iter().enumerate() {
            if b == 0 {
                let s = String::from_utf8_lossy(&buffer[cur_start..i]).into_owned();
                paths.push(s);
                cur_start = i + 1;
                if paths.len() == count { break; }
            }
        }
        while paths.len() < count { paths.push(String::new()); }
        Ok(paths)
    }
}

pub(crate) fn read_long_7bit_from_slice(buf: &[u8], offset: &mut usize, tag_bit: u8, prev_byte: u8) -> i64 {
    let code = if tag_bit != 0 {
        prev_byte
    } else {
        let b = buf[*offset];
        *offset += 1;
        b
    };
    let mask = (1u8 << (7 - tag_bit)).wrapping_sub(1);
    let mut value = (code & mask) as i64;
    if (code & (1 << (7 - tag_bit))) == 0 { return value; }
    loop {
        if (value >> (8 * 8 - 7)) != 0 { return 0; }
        let code = buf[*offset];
        *offset += 1;
        value = (value << 7) | ((code & 0x7F) as i64);
        if (code & 0x80) == 0 { break; }
    }
    value
}

#[derive(Debug, Clone)]
pub(crate) struct ChunkStream<T: Read + Seek> {
    inner: T,
    start: u64,
    end: u64,
    cur_pos: u64,
}

impl<T: Read + Seek> ChunkStream<T> {
    pub fn new(mut stream: T, start: u64, end: u64) -> std::io::Result<Self> {
        if end < start {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "end < start"));
        }
        stream.seek(SeekFrom::Start(start))?;
        Ok(Self { inner: stream, start, end, cur_pos: 0 })
    }

    #[inline]
    fn size(&self) -> u64 { self.end - self.start }

    #[inline]
    fn remain(&self) -> u64 { self.size().saturating_sub(self.cur_pos) }
}

impl<T: Read + Seek> Read for ChunkStream<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remain() == 0 { return Ok(0); }
        let to_read = std::cmp::min(buf.len() as u64, self.remain()) as usize;
        self.inner.seek(SeekFrom::Start(self.start + self.cur_pos))?;
        let n = self.inner.read(&mut buf[..to_read])?;
        self.cur_pos += n as u64;
        Ok(n)
    }
}

impl<T: Read + Seek> Seek for ChunkStream<T> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(offset) => {
                let signed = self.cur_pos as i64 + offset;
                if signed < 0 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek before start")); }
                signed as u64
            }
            SeekFrom::End(offset) => {
                let signed = self.size() as i64 + offset;
                if signed < 0 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek before start")); }
                signed as u64
            }
        };
        if new_pos > self.size() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek beyond end")); }
        self.cur_pos = new_pos;
        self.inner.seek(SeekFrom::Start(self.start + new_pos))?;
        Ok(self.cur_pos)
    }
}

impl CombinedStream {
    pub fn new(streams: Vec<File>) -> std::io::Result<Self> {
        if streams.is_empty() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "streams cannot be empty")); }
        let mut start_positions = vec![0u64; streams.len()];
        for i in 1..streams.len() {
            let prev_len = streams[i - 1].metadata()?.len();
            start_positions[i] = start_positions[i - 1] + prev_len;
        }
        let last_len = streams.last().unwrap().metadata()?.len();
        let total_len = start_positions.last().copied().unwrap_or(0) + last_len;
        Ok(Self { streams, start_positions, position: 0, index: 0, total_length: total_len })
    }

    pub fn from_new_files(new_streams: Vec<NewFileCombinedStream>) -> std::io::Result<Self> {
        if new_streams.is_empty() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "streams cannot be empty")); }
        let mut streams = Vec::with_capacity(new_streams.len());
        let mut start_positions = vec![0u64; new_streams.len()];
        for (i, s) in new_streams.iter().enumerate() {
            s.file.set_len(s.size)?;
            if i > 0 { start_positions[i] = start_positions[i - 1] + new_streams[i - 1].size; }
            streams.push(s.file.try_clone()?);
        }
        let last_size = new_streams.last().unwrap().size;
        let total_len = start_positions.last().copied().unwrap_or(0) + last_size;
        Ok(Self { streams, start_positions, position: 0, index: 0, total_length: total_len })
    }

    pub fn length(&self) -> u64 { self.total_length }
    pub fn get_position(&self) -> u64 { self.position }

    fn update_index(&mut self) -> std::io::Result<()> {
        if self.position == self.total_length {
            self.index = self.streams.len() - 1;
            return Ok(());
        }
        while self.index > 0 && self.position < self.start_positions[self.index] { self.index -= 1; }
        while self.index + 1 < self.streams.len() {
            let cur_end = self.start_positions[self.index] + self.streams[self.index].metadata()?.len();
            if self.position >= cur_end { self.index += 1; } else { break; }
        }
        Ok(())
    }
}

impl Read for CombinedStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let mut result = 0usize;
        let mut remaining = buffer.len();
        let mut offset = 0;

        while remaining > 0 {
            let cur_len = self.streams[self.index].metadata()?.len();
            let pos_in_stream = self.position - self.start_positions[self.index];
            if pos_in_stream >= cur_len {
                if self.index + 1 < self.streams.len() {
                    self.index += 1;
                    continue;
                } else { break; }
            }
            self.streams[self.index].seek(SeekFrom::Start(pos_in_stream))?;
            let bytes_available = (cur_len - pos_in_stream) as usize;
            let to_read = bytes_available.min(remaining);
            let n = self.streams[self.index].read(&mut buffer[offset..offset + to_read])?;
            if n == 0 { break; }
            result += n;
            offset += n;
            remaining -= n;
            self.position += n as u64;
            if remaining > 0 && self.index + 1 < self.streams.len() { self.index += 1; }
        }
        Ok(result)
    }
}

impl Write for CombinedStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let mut total = 0usize;
        let mut remaining = buffer.len();
        let mut offset = 0;

        while remaining > 0 {
            let cur_len = self.streams[self.index].metadata()?.len();
            let pos_in_stream = self.position - self.start_positions[self.index];
            if pos_in_stream >= cur_len {
                if self.index + 1 < self.streams.len() {
                    self.index += 1;
                    continue;
                } else { break; }
            }
            self.streams[self.index].seek(SeekFrom::Start(pos_in_stream))?;
            let capacity = (cur_len - pos_in_stream) as usize;
            let to_write = capacity.min(remaining);
            self.streams[self.index].write_all(&buffer[offset..offset + to_write])?;
            total += to_write;
            offset += to_write;
            remaining -= to_write;
            self.position += to_write as u64;
            if remaining > 0 && self.index + 1 < self.streams.len() { self.index += 1; }
        }
        Ok(total)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        for s in &mut self.streams { s.flush()?; }
        Ok(())
    }
}

impl Seek for CombinedStream {
    fn seek(&mut self, origin: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match origin {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(offset) => {
                let v = self.position as i64 + offset;
                if v < 0 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek before start")); }
                v as u64
            }
            SeekFrom::End(offset) => {
                let v = self.total_length as i64 + offset;
                if v < 0 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek before start")); }
                v as u64
            }
        };
        if new_pos > self.total_length { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek beyond end")); }
        self.position = new_pos;
        self.update_index()?;
        Ok(self.position)
    }
}
