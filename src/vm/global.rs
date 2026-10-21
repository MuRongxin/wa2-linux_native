//! Persistent story-wide variables.
//!
//! The original engine declares these values in `script/Global.vrb` and
//! keeps them outside the per-scene interpreter.  A scene can therefore set a
//! route/event flag and a later scene can read it.  The native port keeps the
//! same indexed table, while storing only the values in a native save file;
//! the schema is always rebuilt from the installed data tree.

use crate::codec::sjis;
use std::path::Path;

/// Schema revision for the native representation.  This is deliberately
/// independent of the save-file version: a changed `Global.vrb` can still be
/// padded/truncated safely when loading an older native save.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalState {
    /// Names are useful in diagnostics and make the indexed model auditable;
    /// they are not written to saves because the data tree is the schema
    /// source of truth.
    pub names: Vec<String>,
    pub defaults: Vec<i32>,
    pub values: Vec<i32>,
}

impl GlobalState {
    /// Load the native equivalent of the original global variable table.
    /// `script_dir` is normally `data/script` or `data/script_cn`.
    pub fn load(script_dir: &Path) -> GlobalState {
        let path = script_dir.join("Global.vrb");
        match std::fs::read(&path) {
            Ok(bytes) => {
                let mut names = Vec::new();
                let mut defaults = Vec::new();
                for line in bytes.split(|b| *b == b'\n' || *b == b'\r') {
                    let line = line.trim_ascii();
                    if line.is_empty() {
                        continue;
                    }
                    let mut fields = line.split(|b| *b == b',');
                    let _ty = fields.next();
                    let Some(raw_name) = fields.next() else { continue };
                    let default = fields
                        .next()
                        .and_then(|v| std::str::from_utf8(v).ok())
                        .and_then(|v| v.trim().parse::<i32>().ok())
                        .unwrap_or(0);
                    let name = sjis::decode(raw_name);
                    if name.is_empty() {
                        continue;
                    }
                    names.push(name);
                    defaults.push(default);
                }
                if !names.is_empty() {
                    let values = defaults.clone();
                    eprintln!(
                        "global: loaded {} variables from {} (schema {})",
                        names.len(),
                        path.display(),
                        SCHEMA_VERSION
                    );
                    return GlobalState { names, defaults, values };
                }
                eprintln!("global: {} has no usable variables, using empty schema", path.display());
            }
            Err(e) => eprintln!("global: cannot read {}: {}, using empty schema", path.display(), e),
        }
        GlobalState { names: Vec::new(), defaults: Vec::new(), values: Vec::new() }
    }

    pub fn empty() -> GlobalState {
        GlobalState { names: Vec::new(), defaults: Vec::new(), values: Vec::new() }
    }

    pub fn reset(&mut self) {
        self.values.clone_from(&self.defaults);
    }

    pub fn get(&self, index: u32) -> i32 {
        self.values.get(index as usize).copied().unwrap_or(0)
    }

    pub fn set(&mut self, index: u32, value: i32) {
        if let Some(dst) = self.values.get_mut(index as usize) {
            *dst = value;
        }
    }

    pub fn snapshot(&self) -> Vec<i32> {
        self.values.clone()
    }

    /// Restore values from a native save. Unknown trailing fields are ignored;
    /// missing fields keep the schema default. This is also the migration path
    /// for a save made before a later `Global.vrb` gained another entry.
    pub fn restore_values(&mut self, saved: &[i32]) {
        self.reset();
        let n = self.values.len().min(saved.len());
        self.values[..n].copy_from_slice(&saved[..n]);
        if saved.len() != self.values.len() {
            eprintln!(
                "global: save has {} values, current schema has {}; padded/truncated safely",
                saved.len(),
                self.values.len()
            );
        }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_schema(contents: &[u8]) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("wa2-global-{}-{}", std::process::id(), stamp));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Global.vrb"), contents).unwrap();
        dir
    }

    #[test]
    fn loads_cp932_schema_and_defaults() {
        let dir = temp_schema(b"3,FLG_00,1\r\n3,SubWait_timer,0\r\n");
        let mut g = GlobalState::load(&dir);
        assert_eq!(g.names, ["FLG_00", "SubWait_timer"]);
        assert_eq!(g.values, [1, 0]);
        g.set(1, 2400);
        assert_eq!(g.get(1), 2400);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restore_pads_and_truncates_without_leaking_old_values() {
        let dir = temp_schema(b"3,A,7\n3,B,8\n3,C,9\n");
        let mut g = GlobalState::load(&dir);
        g.restore_values(&[100, 200]);
        assert_eq!(g.values, [100, 200, 9]);
        g.restore_values(&[1, 2, 3, 4]);
        assert_eq!(g.values, [1, 2, 3]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shipped_global_schema_has_the_expected_route_table() {
        let path = std::path::Path::new("data/script/Global.vrb");
        if !path.is_file() {
            return;
        }
        let g = GlobalState::load(path.parent().unwrap());
        assert_eq!(g.values.len(), 29);
        assert_eq!(g.names.first().map(String::as_str), Some("FLG_00"));
        assert_eq!(g.names.get(1).map(String::as_str), Some("SubWait_timer"));
    }
}
