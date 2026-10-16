//! WA2 scenario model + parser driven from the shipped human-readable
//! scenario text (`.txt`) files.
//!
//! A scenario file is a single comma-separated "page". Layout (from `1001.txt`):
//!   field 0  : empty
//!   field 1  : voice / movie id (e.g. `mv01`)
//!   field 2-3: speaker line + text line
//!   field 4  : background tint reference (e.g. `sepia.AMP`)
//!   field 5+ : narrative / speech text segments (`\\n` = line break,
//!              `<R..>` = region tag)
//!   tail     : `CATCH`,<next-script>
//!
//! The companion `.bnr` file holds the LSCR bytecode that sequences these pages
//! (choices, branches, sprite timing); parsing that is a separate task. This
//! module extracts the readable text + metadata so the renderer can display a
//! real scene.

use crate::codec::sjis;

pub mod lscr;

#[derive(Debug, Default, Clone)]
pub struct ScenarioPage {
    pub voice: Option<String>,
    pub speaker: Option<String>,
    pub background: Option<String>,
    pub next: Option<String>,
    /// Text segments in display order (empty control fields removed).
    pub lines: Vec<String>,
}

impl ScenarioPage {
    /// All text joined with newlines, for rendering into the dialog box.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
}

/// One logical step in a scenario playback sequence.
#[derive(Debug, Clone)]
pub enum Step {
    /// Background / effect resource (e.g. `sepia.AMP`, `B990000.tga`).
    Bg(String),
    /// Full-motion video (e.g. `mv01`).
    Movie(String),
    /// A spoken line with an explicit speaker name.
    ///
    /// `voice_line` is the .txt field index of the speaker token, which
    /// matches the voice filename pattern `{scenario}_{voice_line:04d}_XX.OGG`.
    Say {
        speaker: String,
        text: String,
        voice_line: Option<u32>,
    },
    /// Narration / internal monologue (no speaker).
    Mono(String),
    /// A choice point with the listed options.
    Choice(Vec<String>),
    /// Jump to another script id (from `CATCH` / `CATCH2` / `CATCH3`).
    Jump { target: String, kind: String },
}

/// Full-width digit characters used for choice numbering in WA2 scripts.
const ZEN_DIGITS: &[char] = &['０', '１', '２', '３', '４', '５', '６', '７', '８', '９'];

fn is_zen_digit(c: char) -> bool {
    ZEN_DIGITS.contains(&c)
}

/// Strip a leading full-width digit + `.`/`．` from a choice option, returning
/// the option text.
pub fn strip_choice_prefix(s: &str) -> Option<String> {
    let mut chars = s.chars();
    let first = chars.next()?;
    if !is_zen_digit(first) {
        return None;
    }
    let rest: String = chars.collect();
    let rest = rest.trim_start_matches(['．', '.', ' ']);
    Some(rest.to_string())
}

/// Parse a scenario `.txt` file into a sequence of playback steps.
///
/// Mirrors the logic of `wa2tool/parse_playback.py`: resource tokens become
/// `Bg`/`Movie`, quoted lines become `Say` (with a remembered speaker),
/// narration becomes `Mono`, numbered options become `Choice`, and `CATCH*`
/// tokens become `Jump`.
pub fn parse_steps(bytes: &[u8], cn: bool) -> Vec<Step> {
    let s = if cn { sjis::decode_cn(bytes) } else { sjis::decode(bytes) };
    let toks: Vec<&str> = s.split(',').collect();
    let mut steps: Vec<Step> = Vec::new();
    let mut cur_speaker = String::new();
    let mut i = 0;
    let n = toks.len();

    while i < n {
        let t = toks[i].trim_matches(['\r', '\n']);
        let s = t.trim();

        // ---- jump commands ----
        if s == "CATCH" || s == "CATCH2" || s == "CATCH3" {
            let tgt = if i + 1 < n {
                toks[i + 1].trim_matches(['\r', '\n']).trim().to_string()
            } else {
                String::new()
            };
            // A valid WA2 script id is digits optionally followed by _NNN(_N).
            let valid = !tgt.is_empty()
                && tgt
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '_');
            if valid {
                steps.push(Step::Jump {
                    target: tgt,
                    kind: s.to_string(),
                });
            }
            i += 2;
            continue;
        }

        // ---- choices (１．... ２．...) ----
        if let Some(opt) = strip_choice_prefix(s) {
            let mut opts = vec![opt];
            let mut j = i + 1;
            while j < n {
                let s2 = toks[j].trim_matches(['\r', '\n']).trim();
                if let Some(o2) = strip_choice_prefix(s2) {
                    opts.push(o2);
                    j += 1;
                } else {
                    break;
                }
            }
            if opts.len() >= 2 {
                steps.push(Step::Choice(opts));
                i = j;
                continue;
            }
        }

        // ---- movie ids ----
        if s.starts_with("mv") && s[2..].chars().all(|c| c.is_ascii_digit()) {
            steps.push(Step::Movie(s.to_string()));
            i += 1;
            continue;
        }

        // ---- resource files ----
        let low = s.to_ascii_lowercase();
        if low.ends_with(".tga") || low.ends_with(".amp") || low.ends_with(".bmp") {
            steps.push(Step::Bg(s.to_string()));
            i += 1;
            continue;
        }
        if low.ends_with(".wav") || low.ends_with(".ogg") || low.ends_with(".pak") {
            // Audio/other resource: skip for now (mixer handles these separately).
            i += 1;
            continue;
        }

        // ---- ascii commands / ids to ignore ----
        if s.is_empty()
            || is_command_token(s)
            || (s.is_ascii()
                && s.len() < 24
                && !s.chars().all(|c| c.is_ascii_digit())
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'))
        {
            i += 1;
            continue;
        }
        if s.starts_with('<') && s.ends_with('>') {
            i += 1;
            continue;
        }

        // ---- text ----
        if !s.is_empty() {
            // A short name-like token followed by a quoted line is a speaker.
            if is_name_like(s) && !is_quoted(s) && i + 1 < n {
                let nxt = toks[i + 1].trim_matches(['\r', '\n']).trim();
                if !nxt.is_empty() && (nxt.starts_with('「') || nxt.starts_with('『') || is_quoted(nxt)) {
                    steps.push(Step::Say {
                        speaker: s.to_string(),
                        text: nxt.to_string(),
                        voice_line: Some(i as u32),
                    });
                    cur_speaker = s.to_string();
                    i += 2;
                    continue;
                }
            }
            // Standalone quoted line uses the last known speaker.
            if s.starts_with('「') || s.starts_with('『') {
                steps.push(Step::Say {
                    speaker: cur_speaker.clone(),
                    text: s.to_string(),
                    voice_line: Some(i as u32),
                });
                i += 1;
                continue;
            }
            // Narration / monologue.
            steps.push(Step::Mono(s.to_string()));
            i += 1;
            continue;
        }
        i += 1;
    }
    steps
}

fn is_command_token(s: &str) -> bool {
    matches!(
        s,
        "bak" | "grp" | "sepia" | "BGM" | "SE" | "VOICE" | "WAIN" | "WSET"
            | "PRINT" | "PRINT2" | "PRINT3" | "NEXT" | "BRANCH" | "RETURN"
            | "CALL" | "JUMP" | "RESTART" | "TITLE" | "FLG" | "FLAG" | "IF"
            | "ELSE" | "ENDIF" | "RAND" | "SELECT"
    )
}

/// True if the string is surrounded by Japanese quotation marks.
fn is_quoted(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 6 {
        return false;
    }
    (s.starts_with('「') && s.ends_with('」')) || (s.starts_with('『') && s.ends_with('』'))
}

/// A name-like token: short, no sentence-ending punctuation.
///
/// The `？` character is allowed because WA2 uses `？？？` as the "unknown
/// speaker" marker.
fn is_name_like(s: &str) -> bool {
    if s.is_empty() || s.chars().count() > 14 {
        return false;
    }
    !s.chars().any(|c| {
        matches!(
            c,
            '。' | '、' | '！' | '…' | '「' | '」' | '『' | '』' | '\n' | ' '
        )
    })
}

/// Parse a scenario `.txt` file (raw CP932 bytes) into a flat page.
///
/// Retained for the `text-info` / simple `scene` commands. For playback with
/// choices and jumps use [`parse_steps`].
pub fn parse_scenario_text(bytes: &[u8]) -> ScenarioPage {
    let s = sjis::decode(bytes);
    let fields: Vec<&str> = s.split(',').collect();
    let mut page = ScenarioPage::default();

    if fields.len() > 1 {
        let v = fields[1].trim();
        if v.starts_with("mv") {
            page.voice = Some(v.to_string());
        }
    }
    if fields.len() > 2 {
        let sp = fields[2].trim();
        if !sp.is_empty() {
            page.speaker = Some(sp.to_string());
        }
    }
    if fields.len() > 4 {
        let bg = fields[4].trim();
        if bg.to_ascii_uppercase().ends_with(".AMP") {
            page.background = Some(bg.to_string());
        }
    }

    // Text fields: everything from index 5 until CATCH, minus empties and the
    // pure-speaker placeholder lines.
    let mut i = 5;
    while i < fields.len() {
        let f = fields[i].trim();
        if f == "CATCH" {
            if i + 1 < fields.len() {
                page.next = Some(fields[i + 1].trim().to_string());
            }
            break;
        }
        if !f.is_empty() {
            // skip speaker-only placeholders like '？？？'
            let speaker_only = f.chars().all(|c| c == '？' || c == '?' || c == ' ');
            if !speaker_only {
                page.lines.push(f.to_string());
            }
        }
        i += 1;
    }
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_prefix_works() {
        assert_eq!(strip_choice_prefix("１．苦手だ"), Some("苦手だ".to_string()));
        assert_eq!(strip_choice_prefix("２. なんとなく"), Some("なんとなく".to_string()));
        assert_eq!(strip_choice_prefix("普通の文"), None);
    }

    #[test]
    fn parse_simple_scene() {
        // Tokens are ASCII / latin1-safe so sjis::decode round-trips them.
        let raw = ",mv01,sepia.AMP,CATCH,1002";
        let steps = parse_steps(raw.as_bytes(), false);
        assert!(matches!(steps[0], Step::Movie(_)));
        assert!(matches!(steps[1], Step::Bg(_)));
        match &steps[2] {
            Step::Jump { target, .. } => assert_eq!(target, "1002"),
            _ => panic!("expected Jump"),
        }
    }
}
