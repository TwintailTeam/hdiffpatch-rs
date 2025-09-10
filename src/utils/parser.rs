use std::io::{self, Read, BufReader, Cursor, Seek, SeekFrom};
use std::fmt;
use std::fs::File;
use crate::utils::VarInt;

pub trait Byte: Copy + PartialEq + Sized + fmt::Debug {
    fn from_byte(b: u8) -> Self;
    fn to_byte(self) -> u8;
}

impl Byte for u8 {
    fn from_byte(b: u8) -> Self { b }
    fn to_byte(self) -> u8 { self }
}

pub struct Parser<R: Read + Seek> {
    reader: BufReader<R>,
    context: String,
}

impl Parser<File> {
    pub fn from_path(path: &str, context: Option<&str>) -> Self {
        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let context_str = context.unwrap_or("").to_string();
        Self { reader, context: context_str }
    }
}

impl<R: Read + Seek> Parser<R> {
    pub fn from_reader(reader: R, context: &str) -> Self {
        Self { reader: BufReader::new(reader), context: context.to_string() }
    }

    pub fn position(&mut self) -> u64 {
        self.reader.seek(SeekFrom::Current(0)).expect("Failed to get position")
    }

    #[inline(never)]
    fn error(&self, message: &str) -> ! {
        panic!("Error in {}: {}", self.context, message);
    }

    pub fn check_read(&mut self, b: u64) {
        let pos = self.position();
        if pos != b { self.error(&format!("Expected to read {} bytes, but position is {}", b, pos)); }
    }

    pub fn read_maybe_compressed(&mut self, size: u64, compressed_size: u64) -> Vec<u8> {
        if compressed_size > 0 {
            let compressed_data = self.read_bytes(compressed_size as usize);
            let decompressed = zstd::stream::decode_all(Cursor::new(&compressed_data)).unwrap_or_else(|e| self.error(&format!("Decompression failed: {}", e)));
            if decompressed.len() as u64 != size { self.error("Invalid compressed data: size mismatch"); }
            decompressed
        } else {
            let mut data = vec![0u8; size as usize];
            self.reader.read_to_end(&mut data).unwrap();
            data
        }
    }

    pub fn sub_parser<'a>(&self, data: &'a [u8], context: &str) -> Parser<Cursor<&'a [u8]>> {
        Parser::from_reader(Cursor::new(data), context)
    }

    pub fn read_varint(&mut self, k_tag_bit: u8) -> VarInt {
        let mut byte = self.read::<u8>();
        let mut res = (byte & ((1 << (7 - k_tag_bit)) - 1)) as i64;
        let has_more = (byte & (1 << (7 - k_tag_bit))) != 0;
        let sign = k_tag_bit > 0 && (byte & 0x80) != 0;

        if has_more {
            loop {
                byte = self.read::<u8>();
                let next_bits = (byte & 0x7F) as i64;
                res = (res << 7) | next_bits;
                if (byte & 0x80) == 0 {
                    break;
                }
            }
        }

        if sign {
            VarInt(-res)
        } else {
            VarInt(res)
        }
    }

    pub fn match_varint(&mut self, expected: u64) {
        let val = self.read_varint(expected as u8);
        if val.0 != expected as i64 {
            self.error(&format!("Expected varint {}, got {:?}", expected, val));
        }
    }

    pub fn read<T: Byte>(&mut self) -> T {
        let mut buf = [0u8; 1];
        if self.reader.read_exact(&mut buf).is_err() {
            self.error("Unexpected EOF while reading byte");
        }
        T::from_byte(buf[0])
    }

    pub fn read_bytes<T: Byte>(&mut self, n: usize) -> Vec<T> {
        let mut buf = vec![0u8; n];
        if self.reader.read_exact(&mut buf).is_err() {
            self.error(&format!("Unexpected EOF while reading {} bytes", n));
        }
        buf.into_iter().map(T::from_byte).collect()
    }

    pub fn read_string(&mut self) -> String {
        let mut buffer = Vec::new();
        if self.read_until::<u8>(0, &mut buffer).is_err() {
            self.error("Unexpected EOF while reading string");
        }
        if let Some(0) = buffer.last() {
            buffer.pop();
        }
        String::from_utf8(buffer).unwrap_or_else(|_| self.error("Invalid UTF-8 string"))
    }

    pub fn read_until<T: Byte>(&mut self, c: T, buffer: &mut Vec<u8>) -> io::Result<()> {
        let mut byte = [0u8];
        loop {
            let n = self.reader.read(&mut byte)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF reached"));
            }
            if byte[0] == c.to_byte() {
                break;
            }
            buffer.push(byte[0]);
        }
        Ok(())
    }

    pub fn match_byte<T: Byte>(&mut self, expected: T) {
        let r = self.read::<T>();
        if r != expected {
            self.error(&format!("Expected {}, got {}", expected.to_byte(), r.to_byte()));
        }
    }

    pub fn match_bytes<T: Byte>(&mut self, expected: &[T]) {
        let bytes = self.read_bytes::<T>(expected.len());
        if bytes != expected {
            self.error(&format!(
                "Expected '{:?}', got '{:?}'",
                expected.iter().map(|b| b.to_byte()).collect::<Vec<_>>(),
                bytes.iter().map(|b| b.to_byte()).collect::<Vec<_>>()
            ));
        }
    }
}
