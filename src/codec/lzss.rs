//! Okumura LZSS decompression (N=4096, F=18, THRESHOLD=2).
//! Ported from wa2repack's lzss.cpp (unlzss / lzss_read), which is the
//! exact algorithm used by White Album 2's KCAP archives.

const N: usize = 4096;
const F: usize = 18;
const THRESHOLD: usize = 2;

/// Decompress `input` (the bytes after the 8-byte DATAHDR) into a buffer of
/// exactly `output_len` bytes.
pub fn decompress(input: &[u8], output_len: usize) -> Vec<u8> {
    let mut text_buf = [0u8; N + F - 1];
    for c in 0..(N - F) {
        text_buf[c] = 0;
    }
    let mut r = N - F;
    let mut flags: u32 = 0;
    let mut input_index = 0usize;
    let mut buf = Vec::with_capacity(output_len);

    'outer: loop {
        flags >>= 1;
        if (flags & 256) == 0 {
            if input_index >= input.len() {
                break;
            }
            let c = input[input_index];
            input_index += 1;
            flags = (c as u32) | 0xFF00;
        }

        if (flags & 1) != 0 {
            // literal byte
            if input_index >= input.len() {
                break;
            }
            let c = input[input_index];
            input_index += 1;
            text_buf[r] = c;
            r = (r + 1) & (N - 1);
            buf.push(c);
            if buf.len() >= output_len {
                break;
            }
        } else {
            // position+length pair
            if input_index >= input.len() {
                break;
            }
            let i0 = input[input_index];
            input_index += 1;
            if input_index >= input.len() {
                break;
            }
            let j0 = input[input_index];
            input_index += 1;
            let i = (i0 as usize) | (((j0 as usize) & 0xF0) << 4);
            let j = ((j0 as usize) & 0x0F) + THRESHOLD;
            for k in 0..=j {
                let c = text_buf[(i + k) & (N - 1)];
                text_buf[r] = c;
                r = (r + 1) & (N - 1);
                buf.push(c);
                if buf.len() >= output_len {
                    break 'outer;
                }
            }
        }
    }
    buf
}
