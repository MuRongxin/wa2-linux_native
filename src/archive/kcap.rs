//! KCAP archive unpacker for White Album 2 (Leaf AVG32 engine).
//!
//! File layout (from wa2repack's kcap_types.h / unpack.cpp):
//!   KCAPHDR  : signature[4]="KCAP", unknown1(u32), unknown2(u32), entry_count(u32)  -> 16 bytes
//!   KCAPENTRY: compressed_flag(u32), filename[24], unknown1(u32), unknown2(u32),
//!              offset(u32), length(u32)                                     -> 44 bytes
//!   For a compressed entry: at `offset` there is a DATAHDR { length(u32),
//!   original_length(u32) } (8 bytes), then `length - 8` bytes of LZSS-compressed
//!   data. The result is `original_length` bytes.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::codec::lzss;

pub struct Entry {
    pub compressed: bool,
    pub raw_name: Vec<u8>,
    pub offset: u32,
    pub length: u32,
}

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Build a safe output path from the (Shift-JIS) archive filename, preserving
/// any subdirectory components but stripping path traversal.
fn out_path(out_dir: &Path, raw_name: &[u8]) -> PathBuf {
    let mut pb = out_dir.to_path_buf();
    for comp in raw_name.split(|&b| b == b'/' || b == b'\\') {
        if comp.is_empty() || comp == b".." {
            continue;
        }
        pb.push(std::ffi::OsStr::from_bytes(comp));
    }
    pb
}

/// Read and decompress a single archive entry by (ASCII, case-insensitive)
/// name. Used to load character sprites directly out of `char.pak` at runtime.
pub fn read_entry(pak: &Path, name: &str) -> Option<Vec<u8>> {
    let mut f = File::open(pak).ok()?;
    let mut hdr = [0u8; 16];
    f.read_exact(&mut hdr).ok()?;
    if &hdr[0..4] != b"KCAP" {
        return None;
    }
    let entry_count = le_u32(&hdr[12..16]) as usize;
    let want = name.to_ascii_lowercase();
    for _ in 0..entry_count {
        let mut e = [0u8; 44];
        f.read_exact(&mut e).ok()?;
        let name_end = e[4..28].iter().position(|&b| b == 0).unwrap_or(24);
        let raw_name = &e[4..4 + name_end];
        let nm = String::from_utf8_lossy(raw_name).to_ascii_lowercase();
        if nm != want {
            continue;
        }
        let compressed = le_u32(&e[0..4]) != 0;
        let offset = le_u32(&e[36..40]);
        let length = le_u32(&e[40..44]);
        if length == 0 {
            return None;
        }
        f.seek(SeekFrom::Start(offset as u64)).ok()?;
        if compressed {
            let mut dh = [0u8; 8];
            f.read_exact(&mut dh).ok()?;
            let orig = le_u32(&dh[4..8]) as usize;
            let comp_len = (length as usize).saturating_sub(8);
            let mut comp = vec![0u8; comp_len];
            f.read_exact(&mut comp).ok()?;
            return Some(lzss::decompress(&comp, orig));
        } else {
            let mut d = vec![0u8; length as usize];
            f.read_exact(&mut d).ok()?;
            return Some(d);
        }
    }
    None
}

/// List every entry name in the archive (ASCII, lowercased), without
/// decompressing any payload. Used by the sprite bank to build a fast lookup
/// index so it can resolve `(char, series, variant)` against real filenames.
pub fn list_names(pak: &Path) -> Vec<String> {
    let mut f = match File::open(pak) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut hdr = [0u8; 16];
    if f.read_exact(&mut hdr).is_err() || &hdr[0..4] != b"KCAP" {
        return Vec::new();
    }
    let entry_count = le_u32(&hdr[12..16]) as usize;
    let mut names = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let mut e = [0u8; 44];
        if f.read_exact(&mut e).is_err() {
            break;
        }
        let name_end = e[4..28].iter().position(|&b| b == 0).unwrap_or(24);
        let raw_name = &e[4..4 + name_end];
        let nm = String::from_utf8_lossy(raw_name).to_ascii_lowercase();
        if !nm.is_empty() {
            names.push(nm);
        }
    }
    names
}

pub fn unpack(pak: &Path, out_dir: &Path) -> io::Result<()> {
    let mut f = File::open(pak)?;
    let mut hdr = [0u8; 16];
    f.read_exact(&mut hdr)?;
    if &hdr[0..4] != b"KCAP" {
        // Not a KCAP archive (LAC audio / WMV video live in other files).
        println!(
            "{}: not a KCAP archive (likely LAC audio or WMV video), skipping",
            pak.display()
        );
        return Ok(());
    }
    let entry_count = le_u32(&hdr[12..16]) as usize;
    println!("KCAP {}: {} entries", pak.display(), entry_count);

    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let mut e = [0u8; 44];
        f.read_exact(&mut e)?;
        let compressed_flag = le_u32(&e[0..4]);
        let name_end = e[4..28].iter().position(|&b| b == 0).unwrap_or(24);
        let raw_name = e[4..4 + name_end].to_vec();
        let offset = le_u32(&e[36..40]);
        let length = le_u32(&e[40..44]);
        entries.push(Entry {
            compressed: compressed_flag != 0,
            raw_name,
            offset,
            length,
        });
    }

    fs::create_dir_all(out_dir)?;
    let mut ok = 0usize;
    for (idx, ent) in entries.iter().enumerate() {
        if ent.length == 0 {
            continue;
        }
        f.seek(SeekFrom::Start(ent.offset as u64))?;
        let data = if ent.compressed {
            let mut dh = [0u8; 8];
            f.read_exact(&mut dh)?;
            let _dh_len = le_u32(&dh[0..4]);
            let orig = le_u32(&dh[4..8]) as usize;
            let comp_len = (ent.length as usize).saturating_sub(8);
            let mut comp = vec![0u8; comp_len];
            f.read_exact(&mut comp)?;
            lzss::decompress(&comp, orig)
        } else {
            let mut d = vec![0u8; ent.length as usize];
            f.read_exact(&mut d)?;
            d
        };

        let path = out_path(out_dir, &ent.raw_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&path)?;
        out.write_all(&data)?;
        println!(
            "  [{}] {} ({} bytes)",
            idx,
            String::from_utf8_lossy(&ent.raw_name),
            data.len()
        );
        ok += 1;
    }
    println!("extracted {} files -> {}", ok, out_dir.display());
    Ok(())
}
