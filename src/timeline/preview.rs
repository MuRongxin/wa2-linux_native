//! The bottom preview bar: the selected scenario's sampled line, decoded for
//! the active language and laid out with the vector font.
//!
//! The chart title, hints and chapter labels use the vector font already. The
//! preview used to call the old bitmap font, so the bar mixed two type
//! systems. Everything here goes through render::text.

use super::*;
use crate::codec::sjis;
use crate::render;

/// Pre-rasterized preview text for one selected node.
pub(super) struct Preview {
    /// Chapter badge, e.g. "终章 · 小春".
    pub badge: Option<Label>,
    /// Scenario id shown next to the badge.
    pub scene: Option<Label>,
    /// Up to two vector lines of the sampled dialogue.
    pub quote: Vec<Label>,
    /// Operation hint under the badge.
    pub hint: Option<Label>,
    /// The sampled line as text (tests and debugging).
    pub line: String,
}

/// Rasterize one line into its own transparent RGBA buffer.
pub(super) fn label(text: &str, size: u32) -> Option<Label> {
    let (w, h, rgba) = render::text::with_ui_font(size, |f| f.line_rgba(text, size)).flatten()?;
    Some(Label { rgba, w, h })
}

/// Sampled preview line for a scenario, decoded for the active language.
///
/// The Chinese patch keeps Shift-JIS code points and repaints the glyphs, so a
/// plain CP932 decode returns unrelated kanji - in Chinese mode this has to go
/// through sjis::decode_cn, exactly like the dialogue body.
pub(super) fn line(script_dir: &Path, id: &str, cn: bool) -> String {
    let Ok(bytes) = std::fs::read(script_dir.join(format!("{id}.txt"))) else {
        return String::new();
    };
    let text = if cn { sjis::decode_cn(&bytes) } else { sjis::decode(&bytes) };
    for raw in text.split(',') {
        let t = raw.trim().trim_matches(['\r', '\n', ' ']);
        if t.chars().count() < 4 || t.chars().all(|c| c.is_ascii()) {
            continue;
        }
        if t.starts_with("CATCH") {
            continue;
        }
        let flat = t.replace("\\n", " ").replace(['\n', '\r'], " ");
        let flat = strip_tags(&flat);
        let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.chars().count() < 4 {
            continue;
        }
        return flat.chars().take(18).collect();
    }
    String::new()
}

/// Build the preview text for one node. Called when the selection changes.
pub(super) fn build(script_dir: &Path, node: &Node, cn: bool) -> Preview {
    let hint = label("滚轮缩放 · 拖动画面 · 回车进入 · R 重置 · F 当前", 16);
    let badge = label(node.chapter.title, 20);
    let scene = label(&node.id, 20);
    let line = line(script_dir, &node.id, cn);
    Preview { badge, scene, quote: Vec::new(), hint, line }
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '<' {
            for d in chars.by_ref() {
                if d == '>' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sampled_line_decodes_as_chinese_in_cn_mode() {
        let dir = Path::new("data/script_cn");
        if !dir.is_dir() {
            return;
        }
        let jp = Path::new("data/script");
        let cn_line = line(dir, "3211", true);
        let raw = line(jp, "3211", false);
        assert!(!cn_line.is_empty(), "no preview text for 3211");
        assert!(!raw.is_empty(), "the Japanese line should still decode");
        // A plain CP932 read of the Chinese patch is unrelated kanji; the
        // glyph-table reading must not match it.
        let gibberish = sjis::decode(&std::fs::read(dir.join("3211.txt")).unwrap());
        assert!(gibberish.contains('腿') || gibberish.contains('挨'));
        assert!(!cn_line.contains('腿') && !cn_line.contains('挨'), "{cn_line}");
    }

    #[test]
    fn tags_and_control_tokens_are_stripped() {
        assert_eq!(strip_tags("a<F16b>c"), "ac", "the whole tag, text included, is removed");
    }
}
