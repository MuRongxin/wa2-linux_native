//! The bottom preview bar: the selected scenario's sampled line, decoded for
//! the active language and laid out with the vector font.
//!
//! The chart title, hints and chapter labels use the vector font already. The
//! preview used to call the old bitmap font, so the bar mixed two type
//! systems. Everything here goes through render::text.
//!
//! Layout (paint draws the bar, this module owns the typography):
//!   badge  y = PREVIEW_TOP + 14   chapter title, tinted with the route colour
//!   hint   y = PREVIEW_TOP + 42   one line, two-stage click + keys
//!   quote  y = PREVIEW_TOP + 92   one or two vector lines, 20px, flat UI ink
//! The quote starts at x=256 (after the 208px thumbnail) and must stay left of
//! the minimap at x=1040, so the block is at most 774px wide. Line breaks are
//! computed once per selection change by measuring with the same font that
//! rasterizes the labels.

use super::*;
use crate::codec::sjis;
use crate::render;
use crate::render::text::VectorFont;

/// Width of the text column the preview bar gives the preview (paint starts it
/// at x=256; the minimap starts at x=1040).
const TEXT_MAX_W: u32 = 774;
/// Quote typography: ~20px flat UI ink, no dialogue outline.
const QUOTE_SIZE: u32 = 20;
/// Hint line: one line under the badge. 16px normally, 14px if it would run
/// into the minimap.
const HINT_SIZE: u32 = 16;
const HINT_MIN_SIZE: u32 = 14;
const HINT_TEXT_CN: &str =
    "单击选中 · 再点或回车进入 · 滚轮缩放 · 拖动 · R 重置 · F 当前 · Tab 章节";
/// The hint follows the language mode; the chapter labels come from the
/// chapter table and stay as they are.
const HINT_TEXT_JP: &str =
    "クリックで選択 · 再クリックか Enter で開始 · ホイール拡大 · ドラッグ · R リセット · F 現在地 · Tab 章";
/// Chapter badge and scene id size, matching the chapter headers.
const HEAD_SIZE: u32 = 20;

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
    /// The sampled line as text (tests and debugging). The bar draws the
    /// rasterized labels, so a release build does not read this back.
    #[allow(dead_code)]
    pub line: String,
}

/// Rasterize one line into its own transparent RGBA buffer.
pub(super) fn label(text: &str, size: u32) -> Option<Label> {
    let (w, h, rgba) = render::text::with_ui_font(size, |f| f.line_rgba(text, size)).flatten()?;
    Some(Label { rgba, w, h })
}

/// Build the preview text for one node. Called when the selection changes, so
/// all the measuring and rasterizing here happens once per selection, not per
/// frame.
pub(super) fn build(script_dir: &Path, node: &Node, cn: bool) -> Preview {
    let badge = label(node.chapter.title, HEAD_SIZE);
    let scene = label(&node.id, HEAD_SIZE);
    let line = line(script_dir, &node.id, cn);
    let quote = quote_labels(&line, QUOTE_SIZE, TEXT_MAX_W);
    let hint = fit_label(hint_text(cn), HINT_SIZE, HINT_MIN_SIZE, TEXT_MAX_W);
    Preview { badge, scene, quote, hint, line }
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
    sample(&text)
}

/// First real line of a decoded scenario string pool.
///
/// The pool starts with resource tokens (background/CG names, separators like
/// "bak", movie and palette files) and command strings; the prose follows.
/// A speaker name is a short token in front of a quoted line, so a dialogue
/// takes the token before it as its speaker when that token looks like a name.
fn sample(text: &str) -> String {
    let tokens: Vec<String> = text.split(',').map(clean).collect();
    for (i, token) in tokens.iter().enumerate() {
        if token.is_empty() || is_control(token) {
            continue;
        }
        let dialogue = starts_dialogue(token);
        if dialogue {
            if !has_word(token) || token.chars().count() < 4 {
                continue;
            }
            // The nearest preceding token may be the speaker's name.
            let speaker = tokens[..i]
                .iter()
                .rev()
                .find(|t| !t.is_empty() && !is_control(t))
                .filter(|name| is_speaker_name(name));
            return match speaker {
                Some(name) => format!("{name}：{token}"),
                None => token.clone(),
            };
        }
        // A name token that introduces a quoted line: skip it, the dialogue
        // branch picks it up as the speaker.
        let next = tokens[i + 1..].iter().find(|t| !t.is_empty() && !is_control(t));
        if is_name(token) && next.map(|t| starts_dialogue(t)).unwrap_or(false) {
            continue;
        }
        if has_word(token) && token.chars().count() >= 4 {
            return token.clone();
        }
    }
    String::new()
}

/// Normalize one pool token: control escapes to spaces, ruby tags out,
/// whitespace collapsed. Full-width spaces separate words like a normal space.
fn clean(raw: &str) -> String {
    let flat = raw.replace("\\n", " ").replace("\\k", " ");
    let flat = strip_tags(&flat);
    flat.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Tokens that name resources or commands rather than carry text.
fn is_control(token: &str) -> bool {
    token.is_empty()
        || token.starts_with("CATCH")
        || token.chars().all(|c| c.is_ascii())
        || !has_word(token)
}

/// True when the token contains a letter or digit (a line made only of
/// punctuation or ellipsis dots is not a useful preview).
fn has_word(token: &str) -> bool {
    token.chars().any(|c| c.is_alphanumeric())
}

fn starts_dialogue(token: &str) -> bool {
    token.starts_with('「') || token.starts_with('『')
}

/// Short, unpunctuated token: a speaker name in front of a quoted line.
fn is_name(token: &str) -> bool {
    let n = token.chars().count();
    (1..=8).contains(&n)
        && !token.is_ascii()
        && token.chars().all(|c| c.is_alphanumeric())
        && !starts_dialogue(token)
}

/// Speaker names are only prefixed when short - "男子学生１" is a name too,
/// but a long one would crowd the quote it introduces.
fn is_speaker_name(token: &str) -> bool {
    let n = token.chars().count();
    (2..=4).contains(&n) && is_name(token)
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

/// Rasterize a single line, stepping down to `min_size` and finally
/// ellipsizing so it never runs into the minimap.
fn fit_label(text: &str, size: u32, min_size: u32, max_w: u32) -> Option<Label> {
    for s in [size, min_size] {
        if render::text::with_ui_font(s, |f| f.measure(text) + 8 <= max_w as i32).unwrap_or(false) {
            return label(text, s);
        }
    }
    render::text::with_ui_font(min_size, |f| {
        f.line_rgba(&fit_with_ellipsis(f, text, max_w as i32 - 8), min_size)
    })
    .flatten()
    .map(|(w, h, rgba)| Label { rgba, w, h })
}

/// Operation hint text for the active language mode.
fn hint_text(cn: bool) -> &'static str {
    if cn {
        HINT_TEXT_CN
    } else {
        HINT_TEXT_JP
    }
}

/// Wrap the quote into at most two vector lines that fit `max_w`, rasterizing
/// each into a coverage label. The second line ends in an ellipsis when the
/// text does not fit.
fn quote_labels(text: &str, size: u32, max_w: u32) -> Vec<Label> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    render::text::with_ui_font(size, |f| {
        let content = max_w as i32 - 8; // line_rgba pads 4px on each side
        wrap_two(f, text.trim(), content)
            .into_iter()
            .filter_map(|line| {
                let (w, h, rgba) = f.line_rgba(&line, size)?;
                Some(Label { rgba, w, h })
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Break characters that end a phrase; the first line prefers to end after one.
const STRONG_BREAKS: &[char] = &['。', '！', '？', '!', '?', '…', '」', '』', '）', ')', '】'];
/// Softer breaks: a wrapped line may also end after a comma or a space.
const WEAK_BREAKS: &[char] = &['，', '、', ',', ';', '；', '：', ':', ' '];
/// Opening marks: a line may break before one, but never after one.
const OPENERS: &[char] = &['「', '『', '（', '(', '【'];
/// Closing marks never start a line if they can ride on the previous one.
const CLOSERS: &[char] = &['」', '』', '）', ')', '】', '，', '。', '、', '！', '？', '…'];

/// Split into at most two lines, the second ellipsized when it still overflows.
fn wrap_two(f: &VectorFont, text: &str, max_w: i32) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    if max_w <= 0 || f.measure(text) <= max_w {
        return vec![text.to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    // Longest prefix that fits.
    let mut end = 1usize;
    let mut width = f.advance_after(None, chars[0]);
    for (i, &ch) in chars.iter().enumerate().skip(1) {
        let adv = f.advance_after(Some(chars[i - 1]), ch);
        if width + adv > max_w {
            break;
        }
        width += adv;
        end = i + 1;
    }
    let cut = choose_break(f, &chars, end, max_w);
    let first: String = chars[..cut].iter().collect::<String>().trim_end().to_string();
    let rest: String = chars[cut..].iter().collect::<String>().trim_start().to_string();
    if rest.is_empty() {
        return vec![first];
    }
    vec![first, fit_with_ellipsis(f, &rest, max_w)]
}

/// Where the first line should stop. Prefer a phrase end near the wrapped
/// column, then a comma/space, and never leave a closing mark at the start of
/// the next line.
fn choose_break(f: &VectorFont, chars: &[char], end: usize, max_w: i32) -> usize {
    // A sentence end is worth giving up part of the line, but not most of
    // it; a mere comma only wins when it is near the wrapped column.
    let strong_min = (end / 3).max(1);
    let weak_min = (end * 2 / 3).max(1);
    let strong = |i: usize| {
        (i < chars.len() && OPENERS.contains(&chars[i]))
            || (i > 0 && (STRONG_BREAKS.contains(&chars[i - 1]) || chars[i - 1] == ' '))
    };
    let weak = |i: usize| i > 0 && WEAK_BREAKS.contains(&chars[i - 1]);
    let mut cut = (strong_min..=end)
        .rev()
        .find(|&i| strong(i))
        .or_else(|| (weak_min..=end).rev().find(|&i| weak(i)))
        .unwrap_or(end);
    while cut < chars.len() && CLOSERS.contains(&chars[cut]) {
        let next = cut + 1;
        if next > end {
            break;
        }
        let prefix: String = chars[..next].iter().collect();
        if f.measure(&prefix) > max_w {
            break;
        }
        cut = next;
    }
    cut.max(1)
}

/// Shorten `text` until it fits `max_w` together with a trailing ellipsis.
fn fit_with_ellipsis(f: &VectorFont, text: &str, max_w: i32) -> String {
    let text = text.trim();
    if f.measure(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let ell = '…';
    let mut best = 0usize;
    let mut width = 0i32;
    for (i, &ch) in chars.iter().enumerate() {
        let prev = if i > 0 { Some(chars[i - 1]) } else { None };
        let adv = f.advance_after(prev, ch);
        let ell_w = f.advance_after(Some(ch), ell);
        if width + adv + ell_w > max_w {
            break;
        }
        width += adv;
        best = i + 1;
    }
    if best == 0 {
        return ell.to_string();
    }
    let mut out: String = chars[..best].iter().collect::<String>().trim_end().to_string();
    out.push(ell);
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

    #[test]
    fn resources_and_command_tokens_are_skipped() {
        // The real pools start with picture/command names: the preview has to
        // reach the prose behind them, not show "B990000.tga" or "bak".
        let pool = ",B990000.tga,,bak,mv01,sepia.AMP,<R机场|这里>的时候起，\\n终于下起来了。";
        assert_eq!(sample(pool), "的时候起， 终于下起来了。");
    }

    #[test]
    fn the_first_real_line_is_taken() {
        assert_eq!(sample(",B990000.tga,,bak,「啊…」,然后第一句旁白。"), "「啊…」");
        assert_eq!(sample(",B990000.tga,,bak,先是一句旁白。,「对白」"), "先是一句旁白。");
        // Punctuation-only tokens are not a preview line.
        assert_eq!(sample(",………,「真正的第一句」"), "「真正的第一句」");
    }

    #[test]
    fn a_short_speaker_name_precedes_its_line() {
        assert_eq!(
            sample(",B990000.tga,,bak,春希,「11月都已经结束了呐」"),
            "春希：「11月都已经结束了呐」"
        );
        // A long name is still a name (not prose) but is not prefixed.
        assert_eq!(
            sample(",B990000.tga,,bak,男性播报员,「那么接下来是新闻」"),
            "「那么接下来是新闻」"
        );
        // The unknown-speaker marker is not a name.
        assert_eq!(sample(",？？？,「春希…君」"), "「春希…君」");
    }

    #[test]
    fn novel_markup_is_flattened() {
        let line = sample("序章\u{3000}二年前\\k\\n\\n\u{3000}冬马和纱，很讨厌天空。");
        assert!(!line.contains('\\'), "{line:?}");
        assert!(!line.contains("\\k") && !line.contains("\\n"), "{line:?}");
        assert!(line.starts_with("序章 二年前"), "{line:?}");
        assert!(line.contains("冬马和纱"), "{line:?}");
    }

    #[test]
    fn japanese_mode_keeps_the_japanese_line() {
        let dir = Path::new("data/script");
        if !dir.is_dir() {
            return;
        }
        let jp = line(dir, "1001", false);
        assert!(jp.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)), "{jp}");
        let cn = line(Path::new("data/script_cn"), "1001", true);
        assert!(cn.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)), "{cn}");
    }

    #[test]
    fn a_quote_wraps_to_at_most_two_lines_inside_the_bar() {
        if render::text::with_ui_font(QUOTE_SIZE, |_| ()).is_none() {
            return;
        }
        let short = "春希：「今天也下雪了。」";
        let one = quote_labels(short, QUOTE_SIZE, TEXT_MAX_W);
        assert_eq!(one.len(), 1);
        assert!(one[0].w as u32 <= TEXT_MAX_W);
        // A long paragraph has to be cut to two lines, the last one ellipsized.
        let long = "长".repeat(200);
        let two = quote_labels(&long, QUOTE_SIZE, TEXT_MAX_W);
        assert_eq!(two.len(), 2, "the preview never shows more than two lines");
        for label in &two {
            assert!(label.w as u32 <= TEXT_MAX_W, "line is {}px, bar is {TEXT_MAX_W}px", label.w);
        }
        // The blocks the paint loop stacks must fit above the bottom edge.
        let top = PREVIEW_TOP + 92;
        let bottom = top + two.iter().map(|l| l.h as i32 + 2).sum::<i32>();
        assert!(bottom <= H, "quote ends at {bottom}, past the {H}px frame");
    }

    #[test]
    fn a_wrapped_line_ends_at_a_phrase_boundary() {
        if render::text::with_ui_font(QUOTE_SIZE, |_| ()).is_none() {
            return;
        }
        // Every sentence is 2 glyphs, so every candidate break is a phrase
        // end: the wrap must stop after a full stop, never before one.
        let pool = "句。".repeat(40);
        let lines = render::text::with_ui_font(QUOTE_SIZE, |f| wrap_two(f, &pool, TEXT_MAX_W as i32 - 8)).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with('。'), "{lines:?}");
        assert!(!lines[1].starts_with('。'), "{lines:?}");
        assert!(lines[1].ends_with('…'), "the rest is ellipsized: {lines:?}");
        render::text::with_ui_font(QUOTE_SIZE, |f| {
            assert!(f.measure(&lines[0]) <= TEXT_MAX_W as i32 - 8, "{lines:?}");
            assert!(f.measure(&lines[1]) <= TEXT_MAX_W as i32 - 8, "{lines:?}");
        });
    }

    #[test]
    fn a_sentence_end_wins_well_before_the_wrapped_column() {
        if render::text::with_ui_font(QUOTE_SIZE, |_| ()).is_none() {
            return;
        }
        // The full stop lands at about a third of the line. The wrap should
        // still prefer it over a later comma, so the first line reads as a
        // whole sentence instead of stopping mid-phrase.
        let text = format!("这是第一句的前半部分到这里。{}", "后面还有很多内容，".repeat(6));
        let lines = render::text::with_ui_font(QUOTE_SIZE, |f| {
            wrap_two(f, &text, TEXT_MAX_W as i32 - 8)
        })
        .unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "这是第一句的前半部分到这里。", "{lines:?}");
    }

    #[test]
    fn the_hint_is_a_single_line_inside_the_bar() {
        if render::text::with_ui_font(HINT_SIZE, |_| ()).is_none() {
            return;
        }
        let hint = build_hint();
        let hint = hint.expect("the hint must rasterize");
        assert!(hint.w as u32 <= TEXT_MAX_W, "hint is {}px", hint.w);
        // One line: not much taller than the font size.
        assert!(hint.h <= HINT_SIZE + 12, "hint height {} for a single line", hint.h);
        assert!(hint_text(true).contains("选中") && hint_text(true).contains("回车"));
        assert!(hint_text(true).contains("Tab") && hint_text(true).contains("拖动"));
        assert!(hint_text(false).contains("Enter") && hint_text(false).contains("Tab"));
    }

    /// Hint helper kept separate so the test above does not depend on a chart.
    fn build_hint() -> Option<Label> {
        fit_label(hint_text(true), HINT_SIZE, HINT_MIN_SIZE, TEXT_MAX_W)
    }

    #[test]
    fn data_scenarios_all_produce_a_preview_line() {
        let dir = Path::new("data/script_cn");
        if !dir.is_dir() {
            return;
        }
        // One scenario from every chapter; the preview must not be blank.
        for id in ["1001", "2019", "2020", "2301", "2401", "2501", "3001", "3101", "3201", "3901", "5000", "5100"] {
            let text = line(dir, id, true);
            assert!(!text.is_empty(), "no preview line for {id}");
            if render::text::with_ui_font(QUOTE_SIZE, |_| ()).is_some() {
                let labels = quote_labels(&text, QUOTE_SIZE, TEXT_MAX_W);
                assert!(!labels.is_empty(), "no rasterized line for {id}");
                assert!(labels.len() <= 2, "{id} produced {} lines", labels.len());
                for label in &labels {
                    assert!(label.w as u32 <= TEXT_MAX_W, "{id} line is {}px", label.w);
                }
            }
        }
    }
}
