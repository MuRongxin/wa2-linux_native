//! Shared resource fixtures for tests; never changes the runtime environment.

use std::path::PathBuf;

/// Use the configured shared data tree, or this checkout's downloaded data.
pub(crate) fn root() -> PathBuf {
    std::env::var_os("WA2_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data"))
}

/// Chinese UI art takes priority, with scaled release trees before originals.
/// Prefer 2x for logical-size UI tests to avoid decoding unnecessary pixels.
/// Keep each directory's scale suffix so the normal loaders can normalize it.
pub(crate) fn bg_dirs() -> Vec<PathBuf> {
    let root = root();
    let dirs: Vec<_> = ["bg_cn", "bg"]
        .into_iter()
        .flat_map(|name| {
            ["_2x", "_4x", "_8x", ""].into_iter().map(move |suffix| format!("{name}{suffix}"))
        })
        .map(|name| root.join(name))
        .filter(|dir| dir.is_dir())
        .collect();
    assert!(
        !dirs.is_empty(),
        "missing background fixtures at {} (set WA2_DATA)",
        root.display()
    );
    dirs
}

/// Graph fixtures deliberately use the original scripts, independent of language.
pub(crate) fn script() -> PathBuf {
    let dir = root().join("script");
    assert!(dir.is_dir(), "missing script fixtures at {} (set WA2_DATA)", dir.display());
    dir
}
