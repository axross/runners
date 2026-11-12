//! writing images as PNG and as an icon file, for the `icons` example and the test that
//! keeps the checked-in icon files equal to the embedded source. The app itself never
//! reads or writes an image file. PNG data is stored, not compressed, so the encoder
//! needs no compressor and its output is the same on every machine.

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

    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
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
