//! LAC (Leaf Audio Container) unpacker for White Album 2.
//!
//! Layout (from arc_unpacker's lac_archive_decoder):
//!   magic       : "LAC\0" (4 bytes)
//!   file_count  : u32 LE
//!   per entry   : name[32] (each byte XOR 0xFF, zero-terminated)
//!                 size  : u32 LE
//!                 offset: u32 LE
//!   file data   : raw bytes at `offset`, `size` long (typically Ogg Vorbis).

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub struct Entry {
    pub name: Vec<u8>,
    pub offset: u32,
    pub size: u32,
}

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

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

pub fn unpack(pak: &Path, out_dir: &Path) -> io::Result<()> {
    let mut f = File::open(pak)?;
    let mut hdr = [0u8; 8];
    f.read_exact(&mut hdr)?;
    if &hdr[0..4] != b"LAC\0" {
        println!(
            "{}: not a LAC archive (likely KCAP or WMV), skipping",
            pak.display()
        );
        return Ok(());
    }
    let file_count = le_u32(&hdr[4..8]) as usize;
    println!("LAC {}: {} entries", pak.display(), file_count);

    let mut entries = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        let mut name_buf = [0u8; 32];
        f.read_exact(&mut name_buf)?;
        // Decode: XOR each byte with 0xFF, stop at NUL.
        let mut name = Vec::new();
        for &b in &name_buf {
            if b == 0 {
                break;
            }
            name.push(b ^ 0xFF);
        }
        let size = le_u32(&{
            let mut s = [0u8; 4];
            f.read_exact(&mut s)?;
            s
        });
        let offset = le_u32(&{
            let mut o = [0u8; 4];
            f.read_exact(&mut o)?;
            o
        });
        entries.push(Entry { name, offset, size });
    }

    fs::create_dir_all(out_dir)?;
    let mut ok = 0usize;
    for (idx, ent) in entries.iter().enumerate() {
        if ent.size == 0 {
            continue;
        }
        f.seek(SeekFrom::Start(ent.offset as u64))?;
        let mut data = vec![0u8; ent.size as usize];
        f.read_exact(&mut data)?;
        let path = out_path(out_dir, &ent.name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&path)?;
        out.write_all(&data)?;
        println!(
            "  [{}] {} ({} bytes)",
            idx,
            String::from_utf8_lossy(&ent.name),
            data.len()
        );
        ok += 1;
    }
    println!("extracted {} audio files -> {}", ok, out_dir.display());
    Ok(())
}
