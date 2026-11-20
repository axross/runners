//! writing images as PNG and as an icon file, for the `icons` example and the test that
//! keeps the checked-in icon files equal to the embedded source. The app itself never
//! reads or writes an image file. Windows' PNG data is stored, not compressed; the
//! Mac's icon, up to 1024 pixels square, is packed with fixed Huffman codes and runs of
//! one byte, which needs no compressor crate. Either way the output is the same on
//! every machine.

use crate::glyphs::Bitmap;

/// the CRC-32 PNG chunks carry.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// the Adler-32 that ends a zlib stream.
fn adler32(bytes: &[u8]) -> u32 {
    let (mut low, mut high) = (1_u32, 0_u32);
    for byte in bytes {
        low = (low + u32::from(*byte)) % 65_521;
        high = (high + low) % 65_521;
    }
    (high << 16) | low
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    let mut covered = kind.to_vec();
    covered.extend_from_slice(data);
    out.extend_from_slice(&covered);
    out.extend_from_slice(&crc32(&covered).to_be_bytes());
}

/// `image` as a PNG file.
#[must_use]
pub fn png(image: &Bitmap) -> Vec<u8> {
    png_rgba(image.size, image.size, &image.rgba)
}

/// red, green, blue and alpha bytes, row by row, as a PNG file of `width` by `height`
/// pixels: 8 bits a channel, no interlacing.
#[must_use]
pub fn png_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let row_length = usize::try_from(width).unwrap_or(0).max(1) * 4;
    let mut raw = Vec::with_capacity(rgba.len() + usize::try_from(height).unwrap_or(0));
    for row in rgba.chunks(row_length) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    // a zlib stream of stored blocks of at most 65535 bytes
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(part) = blocks.next() {
        zlib.push(u8::from(blocks.peek().is_none()));
        let length = u16::try_from(part.len()).unwrap_or(u16::MAX);
        zlib.extend_from_slice(&length.to_le_bytes());
        zlib.extend_from_slice(&(!length).to_le_bytes());
        zlib.extend_from_slice(part);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    png_file(width, height, &zlib)
}

/// `image` as a packed PNG file: each row filtered by the pixel to its left, so a flat
/// area is zero bytes, and the zero runs packed.
#[must_use]
pub fn png_packed(image: &Bitmap) -> Vec<u8> {
    let row_length = usize::try_from(image.size).unwrap_or(0).max(1) * 4;
    let mut raw = Vec::with_capacity(image.rgba.len() + row_length / 4);
    for row in image.rgba.chunks(row_length) {
        raw.push(1);
        let left = std::iter::repeat_n(&0, 4).chain(row.iter());
        raw.extend(
            row.iter()
                .zip(left)
                .map(|(byte, left)| byte.wrapping_sub(*left)),
        );
    }
    let mut zlib = vec![0x78, 0x01];
    zlib.extend(deflate_runs(&raw));
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    png_file(image.size, image.size, &zlib)
}

/// bits written from the least significant end, as deflate packs them.
#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    buffer: u32,
    used: u32,
}

impl Bits {
    fn put(&mut self, value: u32, count: u32) {
        self.buffer |= value << self.used;
        self.used += count;
        while self.used >= 8 {
            self.out.push(u8::try_from(self.buffer & 0xFF).unwrap_or(0));
            self.buffer >>= 8;
            self.used -= 8;
        }
    }

    /// a Huffman code, whose bits deflate stores from the most significant one.
    fn code(&mut self, code: u32, count: u32) {
        self.put(code.reverse_bits() >> (32 - count), count);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.out.push(u8::try_from(self.buffer & 0xFF).unwrap_or(0));
        }
        self.out
    }
}

/// the fixed Huffman code of a literal or length symbol (RFC 1951, 3.2.6).
fn fixed(bits: &mut Bits, symbol: u32) {
    match symbol {
        0..=143 => bits.code(0x30 + symbol, 8),
        144..=255 => bits.code(0x190 + symbol - 144, 9),
        256..=279 => bits.code(symbol - 256, 7),
        _ => bits.code(0xC0 + symbol - 280, 8),
    }
}

/// the first length of each length symbol from 257, and its extra bits.
const LENGTHS: [(u32, u32); 29] = [
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 1),
    (13, 1),
    (15, 1),
    (17, 1),
    (19, 2),
    (23, 2),
    (27, 2),
    (31, 2),
    (35, 3),
    (43, 3),
    (51, 3),
    (59, 3),
    (67, 4),
    (83, 4),
    (99, 4),
    (115, 4),
    (131, 5),
    (163, 5),
    (195, 5),
    (227, 5),
    (258, 0),
];

/// `raw` as one deflate block with fixed codes, where a byte repeated three or more
/// times is a match at distance 1.
fn deflate_runs(raw: &[u8]) -> Vec<u8> {
    let mut bits = Bits::default();
    // the last block, with fixed Huffman codes
    bits.put(1, 1);
    bits.put(1, 2);
    let mut at = 0;
    while let Some(byte) = raw.get(at) {
        let previous = at.checked_sub(1).and_then(|before| raw.get(before));
        let run = previous.map_or(0, |previous| {
            raw.iter()
                .skip(at)
                .take(258)
                .take_while(|each| *each == previous)
                .count()
        });
        if run >= 3 {
            let length = u32::try_from(run).unwrap_or(3);
            let (index, (base, extra)) = LENGTHS
                .iter()
                .enumerate()
                .rev()
                .find(|(_, (base, _))| *base <= length)
                .map_or((0, (3, 0)), |(index, entry)| (index, *entry));
            fixed(&mut bits, 257 + u32::try_from(index).unwrap_or(0));
            bits.put(length - base, extra);
            // distance 1 is distance symbol 0, a five-bit code with no extra bits
            bits.code(0, 5);
            at += run;
        } else {
            fixed(&mut bits, u32::from(*byte));
            at += 1;
        }
    }
    fixed(&mut bits, 256);
    bits.finish()
}

fn png_file(width: u32, height: u32, zlib: &[u8]) -> Vec<u8> {
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// `images` as one icon file whose entries are PNG images, which Windows reads from
/// Vista on. Every image is at most 255 pixels square, so its size fits an entry.
#[must_use]
pub fn ico(images: &[Bitmap]) -> Vec<u8> {
    let pngs: Vec<Vec<u8>> = images.iter().map(png).collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(
        &u16::try_from(images.len())
            .unwrap_or(u16::MAX)
            .to_le_bytes(),
    );
    let mut offset = 6 + 16 * images.len();
    for (image, data) in images.iter().zip(&pngs) {
        let side = u8::try_from(image.size).unwrap_or(0);
        out.extend_from_slice(&[side, side, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_le_bytes());
        out.extend_from_slice(&u32::try_from(offset).unwrap_or(u32::MAX).to_le_bytes());
        offset += data.len();
    }
    for data in pngs {
        out.extend_from_slice(&data);
    }
    out
}

/// `images` as a Mac icon file whose entries are packed PNG images, each under the type
/// macOS reads for its size and, at twice a smaller size, the Retina type too.
#[must_use]
pub fn icns(images: &[Bitmap]) -> Vec<u8> {
    let mut entries: Vec<(&[u8; 4], Vec<u8>)> = Vec::new();
    for image in images {
        let types: &[&[u8; 4]] = match image.size {
            16 => &[b"icp4"],
            32 => &[b"icp5", b"ic11"],
            64 => &[b"icp6", b"ic12"],
            128 => &[b"ic07"],
            256 => &[b"ic08", b"ic13"],
            512 => &[b"ic09", b"ic14"],
            1024 => &[b"ic10"],
            _ => &[],
        };
        let data = png_packed(image);
        for kind in types {
            entries.push((kind, data.clone()));
        }
    }
    let total: usize = 8 + entries
        .iter()
        .map(|(_, data)| 8 + data.len())
        .sum::<usize>();
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&u32::try_from(total).unwrap_or(u32::MAX).to_be_bytes());
    for (kind, data) in entries {
        out.extend_from_slice(kind);
        out.extend_from_slice(
            &u32::try_from(8 + data.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        out.extend_from_slice(&data);
    }
    out
}
