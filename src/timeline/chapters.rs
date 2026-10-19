//! Static chapter table: the single place that maps a scenario number to a
//! chapter, a lane and a colour.
//!
//! The chart used to guess chapters from numeric ranges inline. Everything the
//! chart shows - bands, headers, lane colours, the preview badge and the labels
//! on the branch edges - comes from this table, and the key numbers are frozen
//! by the tests at the bottom.
//!
//! Route identity (verified against the shipped scripts; the review report has
//! 310x/320x the wrong way round - do NOT "fix" it back):
//!   * 31xx is the Kazusa route: 3103 says 「だから、かずさを選んだ。」 and 3109
//!     「五年前に、君じゃなくてかずさを選んだ…」.
//!   * 32xx is the Setsuna route: 3210 is the proposal
//!     「だから俺とつきあってください………一生涯」.
//!   * 2020..2033 is the Setsuna main route of CC, not CC-共通, which ends at
//!     2019 (the four-way choice). 2025 is a real CC-Setsuna ending node: the
//!     2020..2023 scenes each carry a branch to it while 2024 leads on.
//!   * 5000..5004 and 5100..5104 are both electronic novels (电子小说), but two
//!     separate chains, so they get two lanes rather than one shared row.
//!
//! A lane is one row of the chart. Chapters that share a lane must occupy
//! disjoint column segments: the main story (序章/CC/CODA) runs on lane 2, the
//! Setsuna/Mari row is lane 3 (CC-麻理 first, then CODA-雪菜 further right),
//! 小春 lane 1, 千晶 lane 0 and 和纱 lane 4, EXTRA lane 5. The two novels are
//! laid out after the story and take lane 0 / lane 3 at the far right.

use super::{Band, Node};

/// One row/chapter of the chart.
pub struct Chapter {
    pub key: &'static str,
    /// Label shown in the chart header and the preview badge.
    pub title: &'static str,
    /// Short route name for edge labels and lane chips.
    pub short: &'static str,
    /// Row of the chart; lane 0 is the top row.
    pub lane: i32,
    /// Route colour for bands, headers and branch edges.
    pub color: [u8; 4],
    /// Inclusive scenario-number ranges that belong to this chapter.
    pub ranges: &'static [(i32, i32)],
    /// Disconnected extra stories are laid out after the main story.
    pub after_story: bool,
}

const CYAN: [u8; 4] = [90, 210, 230, 255];
const GREEN: [u8; 4] = [111, 207, 127, 255];
const ORANGE: [u8; 4] = [224, 138, 60, 255];
const PURPLE: [u8; 4] = [176, 124, 232, 255];
const GOLD: [u8; 4] = [232, 196, 92, 255];
const BLUE_GREY: [u8; 4] = [128, 150, 180, 255];

/// Story order first (ascending first scenario number), then the disconnected
/// extras. Paint draws the bands in this order; the tests freeze the table.
pub(super) static CHAPTERS: &[Chapter] = &[
    Chapter { key: "ic", title: "序章", short: "序章", lane: 2, color: CYAN, ranges: &[(1001, 1013)], after_story: false },
    Chapter { key: "cc_common", title: "终章 · 共通", short: "CC 共通", lane: 2, color: CYAN, ranges: &[(2001, 2019)], after_story: false },
    Chapter { key: "cc_setsuna", title: "终章 · 雪菜", short: "CC 雪菜", lane: 2, color: CYAN, ranges: &[(2020, 2033)], after_story: false },
    Chapter { key: "cc_koharu", title: "终章 · 小春", short: "小春", lane: 1, color: GREEN, ranges: &[(2301, 2322)], after_story: false },
    Chapter { key: "cc_chiaki", title: "终章 · 千晶", short: "千晶", lane: 0, color: ORANGE, ranges: &[(2401, 2413)], after_story: false },
    Chapter { key: "cc_mari", title: "终章 · 麻理", short: "麻理", lane: 3, color: PURPLE, ranges: &[(2501, 2517)], after_story: false },
    Chapter { key: "coda", title: "CODA", short: "CODA", lane: 2, color: GOLD, ranges: &[(3001, 3024)], after_story: false },
    Chapter { key: "coda_kazusa", title: "CODA · 和纱", short: "和纱", lane: 4, color: GOLD, ranges: &[(3101, 3111)], after_story: false },
    // Same purple row as CC-麻理 (the report's route grouping), but the CODA
    // columns are far to the right of 麻理's, so the two bands never overlap.
    Chapter { key: "coda_setsuna", title: "CODA · 雪菜", short: "雪菜", lane: 3, color: PURPLE, ranges: &[(3201, 3211)], after_story: false },
    Chapter { key: "coda_extra", title: "CODA-EXTRA", short: "EXTRA", lane: 5, color: GOLD, ranges: &[(3901, 3909)], after_story: false },
    Chapter { key: "novel_before", title: "电子小说", short: "小说", lane: 0, color: BLUE_GREY, ranges: &[(5000, 5004)], after_story: true },
    Chapter { key: "novel_after", title: "电子小说", short: "小说", lane: 3, color: BLUE_GREY, ranges: &[(5100, 5104)], after_story: true },
];

/// Fallback for a scenario that the table does not know. Kept out of
/// CHAPTERS so it never draws a band.
static OTHER: Chapter = Chapter {
    key: "other",
    title: "其他",
    short: "其他",
    lane: 5,
    color: BLUE_GREY,
    ranges: &[],
    after_story: false,
};

pub(super) fn other() -> &'static Chapter {
    &OTHER
}

/// Chapter a scenario id belongs to.
pub(super) fn of(id: &str) -> &'static Chapter {
    let n = match super::entry_of(id).parse::<i32>() {
        Ok(n) => n,
        Err(_) => return other(),
    };
    CHAPTERS
        .iter()
        .find(|c| c.ranges.iter().any(|&(a, b)| n >= a && n <= b))
        .unwrap_or_else(other)
}

/// True when the table owns this scenario, i.e. it belongs on the chart.
pub(super) fn wanted(id: &str) -> bool {
    of(id).key != "other"
}

/// One vertical background band per chapter, covering every column and lane
/// that chapter occupies.
pub(super) fn bands(nodes: &[Node]) -> Vec<Band> {
    let mut out = Vec::new();
    for chapter in CHAPTERS {
        let mut it = nodes.iter().filter(|n| std::ptr::eq(n.chapter, chapter));
        let Some(first) = it.next() else { continue };
        let (mut first_col, mut last_col) = (first.col, first.col);
        let (mut min_lane, mut max_lane) = (first.lane, first.lane);
        for node in it {
            first_col = first_col.min(node.col);
            last_col = last_col.max(node.col);
            min_lane = min_lane.min(node.lane);
            max_lane = max_lane.max(node.lane);
        }
        out.push(Band { chapter, first_col, last_col, min_lane, max_lane });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(key: &str) -> &'static Chapter {
        CHAPTERS.iter().find(|c| c.key == key).unwrap_or_else(|| panic!("no chapter {key}"))
    }

    /// The table is data, not a guess: freeze all 12 rows and their order.
    #[test]
    fn the_chapter_table_is_frozen() {
        // key, title, short, lane, ranges, after_story
        let expected: [(&str, &str, &str, i32, &[(i32, i32)], bool); 12] = [
            ("ic", "序章", "序章", 2, &[(1001, 1013)], false),
            ("cc_common", "终章 · 共通", "CC 共通", 2, &[(2001, 2019)], false),
            ("cc_setsuna", "终章 · 雪菜", "CC 雪菜", 2, &[(2020, 2033)], false),
            ("cc_koharu", "终章 · 小春", "小春", 1, &[(2301, 2322)], false),
            ("cc_chiaki", "终章 · 千晶", "千晶", 0, &[(2401, 2413)], false),
            ("cc_mari", "终章 · 麻理", "麻理", 3, &[(2501, 2517)], false),
            ("coda", "CODA", "CODA", 2, &[(3001, 3024)], false),
            ("coda_kazusa", "CODA · 和纱", "和纱", 4, &[(3101, 3111)], false),
            ("coda_setsuna", "CODA · 雪菜", "雪菜", 3, &[(3201, 3211)], false),
            ("coda_extra", "CODA-EXTRA", "EXTRA", 5, &[(3901, 3909)], false),
            ("novel_before", "电子小说", "小说", 0, &[(5000, 5004)], true),
            ("novel_after", "电子小说", "小说", 3, &[(5100, 5104)], true),
        ];
        assert_eq!(CHAPTERS.len(), expected.len());
        for (c, exp) in CHAPTERS.iter().zip(expected) {
            assert_eq!(c.key, exp.0);
            assert_eq!(c.title, exp.1);
            assert_eq!(c.short, exp.2);
            assert_eq!(c.lane, exp.3, "{}", c.key);
            assert_eq!(c.ranges, exp.4, "{}", c.key);
            assert_eq!(c.after_story, exp.5, "{}", c.key);
        }
        // The table is in story order: first scenario number ascends, and the
        // after-story novels come last.
        for pair in CHAPTERS.windows(2) {
            assert!(pair[0].ranges[0].0 < pair[1].ranges[0].0, "{} before {}", pair[0].key, pair[1].key);
            assert!(!pair[0].ranges.is_empty());
        }
        for (i, a) in CHAPTERS.iter().enumerate() {
            for b in &CHAPTERS[i + 1..] {
                for &(a0, a1) in a.ranges {
                    for &(b0, b1) in b.ranges {
                        assert!(a1 < b0 || b1 < a0, "{} overlaps {} ({a0}..{a1} / {b0}..{b1})", a.key, b.key);
                    }
                }
            }
        }
        // The novels are two chains: same title, different lanes, laid out
        // after the story so they can never sit on a story row.
        assert_eq!(chapter("novel_before").title, chapter("novel_after").title);
        assert_ne!(chapter("novel_before").lane, chapter("novel_after").lane);
        assert_eq!((chapter("novel_before").lane, chapter("novel_after").lane), (0, 3));
        // 麻理 and CODA-雪菜 share the purple row (report's route grouping).
        assert_eq!(chapter("cc_mari").lane, chapter("coda_setsuna").lane);
        assert_eq!(chapter("cc_mari").color, PURPLE);
        assert_eq!(chapter("coda_setsuna").color, PURPLE);
        // Route colours: main cyan, 小春 green, 千晶 orange, CODA gold,
        // extras blue-grey.
        for key in ["ic", "cc_common", "cc_setsuna"] {
            assert_eq!(chapter(key).color, CYAN, "{key}");
        }
        assert_eq!(chapter("cc_koharu").color, GREEN);
        assert_eq!(chapter("cc_chiaki").color, ORANGE);
        assert_eq!(chapter("coda").color, GOLD);
        assert_eq!(chapter("coda_kazusa").color, GOLD);
        assert_eq!(chapter("coda_extra").color, GOLD);
        for key in ["novel_before", "novel_after"] {
            assert_eq!(chapter(key).color, BLUE_GREY, "{key}");
        }
    }

    #[test]
    fn route_ids_match_the_shipped_scripts() {
        // The review report suggested 31xx = 雪菜 / 32xx = 和纱. The scripts
        // say the opposite (3103 picks かずさ, 3210 proposes to 雪菜), so this
        // test exists to stop anyone "fixing" the table back to the report.
        assert_eq!(of("3101").key, "coda_kazusa");
        assert_eq!(of("3111").key, "coda_kazusa");
        assert_eq!(of("3201").key, "coda_setsuna");
        assert_eq!(of("3211").key, "coda_setsuna");
        assert_ne!(of("3101").key, "coda_setsuna", "the report's swap is wrong");
        assert_ne!(of("3201").key, "coda_kazusa", "the report's swap is wrong");
        // CODA-EXTRA and the two electronic novels.
        assert_eq!(of("3901").key, "coda_extra");
        assert_eq!(of("3909").key, "coda_extra");
        for n in 5000..=5004 {
            let c = of(&n.to_string());
            assert_eq!(c.title, "电子小说");
            assert_eq!(c.lane, 0);
            assert!(c.after_story);
        }
        for n in 5100..=5104 {
            let c = of(&n.to_string());
            assert_eq!(c.title, "电子小说");
            assert_eq!(c.lane, 3);
            assert!(c.after_story);
        }
        // CC: 共通 ends at the 2019 four-way choice; 2020..2033 is the 雪菜
        // main route, and 2025 (its dead-end ending) stays inside that chapter.
        assert_eq!(of("2019").key, "cc_common");
        for n in 2020..=2033 {
            assert_eq!(of(&n.to_string()).key, "cc_setsuna", "{n}");
        }
        assert_eq!(of("2025").key, "cc_setsuna");
        // Main story boundaries.
        assert_eq!(of("1001").key, "ic");
        assert_eq!(of("1013").key, "ic");
        assert_eq!(of("1008_020").key, "ic", "sub-scripts fold into their entry");
        assert_eq!(of("3001").key, "coda");
        assert_eq!(of("3024").key, "coda");
        // Unknown numbers fall back to "other" and stay off the chart.
        assert_eq!(of("9999").key, "other");
        assert_eq!(of("catch").key, "other");
        assert!(wanted("1001") && wanted("3024") && !wanted("9999"));
    }

    /// The route identity above is not folklore: read it out of the scripts.
    /// (data/script is Japanese; the Chinese patch only repaints the glyphs.)
    #[test]
    fn route_identity_is_verified_against_the_script_text() {
        let dir = std::path::Path::new("data/script");
        if !dir.is_dir() {
            return;
        }
        let text = |id: &str| {
            let bytes = std::fs::read(dir.join(format!("{id}.txt"))).unwrap_or_default();
            crate::codec::sjis::decode(&bytes)
        };
        // 31xx = 和纱: Haruki says he chose Kazusa five years ago.
        assert!(text("3103").contains("だから、かずさを選んだ"), "3103 evidence missing");
        assert!(text("3109").contains("君じゃなくてかずさを選んだ"), "3109 evidence missing");
        // 32xx = 雪菜: the proposal ("let's go out ... for the rest of our lives").
        let proposal = (3201..=3211)
            .map(|n| n.to_string())
            .find(|id| text(id).contains("俺とつきあってください"))
            .expect("no proposal scene in 32xx");
        assert_eq!(of(&proposal).key, "coda_setsuna", "proposal scene {proposal} is not in 32xx");
        // 2025 is a real script, not a phantom branch target.
        assert!(!text("2025").is_empty());
        assert_eq!(of("2025").key, "cc_setsuna");
    }
}
