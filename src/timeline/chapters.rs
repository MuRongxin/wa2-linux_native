//! Static chapter table: the single place that maps a scenario number to a
//! chapter, a lane and a colour.
//!
//! The chart used to guess chapters from numeric ranges inline. Everything the
//! chart shows - bands, headers, lane colours, the preview badge - comes from
//! this table, and the key numbers are asserted by the tests at the bottom.
//!
//! Route identity (verified against the shipped scripts):
//!   * 3101..3111 is the Kazusa route: 3103 says "だから、かずさを選んだ。"
//!     and 3109 "五年前に、君じゃなくてかずさを選んだ…".
//!   * 3201..3211 is the Setsuna route: 3211 is the proposal scene
//!     (俺とつきあってください………一生涯).
//!   * 2020..2033 is the Setsuna main route of CC, not CC-共通, which ends at
//!     2019 (the four-way choice).

use super::{Band, Node};

/// One row/chapter of the chart.
pub(super) struct Chapter {
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

/// Story order first, then the disconnected extras. Paint draws the bands in
/// this order; the tests assert the route identity of the key numbers.
pub(super) static CHAPTERS: &[Chapter] = &[
    Chapter { key: "ic", title: "序章", short: "序章", lane: 2, color: CYAN, ranges: &[(1001, 1013)], after_story: false },
    Chapter { key: "cc_common", title: "终章 · 共通", short: "CC 共通", lane: 2, color: CYAN, ranges: &[(2001, 2019)], after_story: false },
    Chapter { key: "cc_setsuna", title: "终章 · 雪菜", short: "CC 雪菜", lane: 2, color: CYAN, ranges: &[(2020, 2033)], after_story: false },
    Chapter { key: "cc_chiaki", title: "终章 · 千晶", short: "千晶", lane: 0, color: ORANGE, ranges: &[(2401, 2413)], after_story: false },
    Chapter { key: "cc_koharu", title: "终章 · 小春", short: "小春", lane: 1, color: GREEN, ranges: &[(2301, 2322)], after_story: false },
    Chapter { key: "cc_mari", title: "终章 · 麻理", short: "麻理", lane: 3, color: PURPLE, ranges: &[(2501, 2517)], after_story: false },
    Chapter { key: "coda", title: "CODA", short: "CODA", lane: 2, color: GOLD, ranges: &[(3001, 3024)], after_story: false },
    Chapter { key: "coda_setsuna", title: "CODA · 雪菜", short: "雪菜", lane: 1, color: PURPLE, ranges: &[(3201, 3211)], after_story: false },
    Chapter { key: "coda_kazusa", title: "CODA · 和纱", short: "和纱", lane: 4, color: GOLD, ranges: &[(3101, 3111)], after_story: false },
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
        Err(_) => return &OTHER,
    };
    CHAPTERS
        .iter()
        .find(|c| c.ranges.iter().any(|&(a, b)| n >= a && n <= b))
        .unwrap_or(&OTHER)
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

    #[test]
    fn route_ids_match_the_shipped_scripts() {
        // The review report suggested 31xx = 雪菜 / 32xx = 和纱. The scripts
        // say the opposite: 3103 picks かずさ, 3211 is the proposal to 雪菜.
        assert_eq!(of("3101").key, "coda_kazusa");
        assert_eq!(of("3201").key, "coda_setsuna");
        assert_eq!(of("2020").key, "cc_setsuna");
        assert_eq!(of("2033").key, "cc_setsuna");
        assert_eq!(of("2019").key, "cc_common");
        assert_eq!(of("5000").title, "电子小说");
        assert_eq!(of("5104").title, "电子小说");
        assert_eq!(of("3901").key, "coda_extra");
        assert!(wanted("1001") && wanted("3024") && !wanted("9999"));
    }

    #[test]
    fn every_chapter_has_a_lane_and_a_colour() {
        let mut keys = std::collections::HashSet::new();
        for c in CHAPTERS {
            assert!(keys.insert(c.key), "duplicate chapter key {}", c.key);
            assert!(!c.title.is_empty() && !c.short.is_empty());
            assert!((0..=5).contains(&c.lane), "{} lane {}", c.key, c.lane);
            assert_eq!(c.color[3], 255);
            assert!(!c.ranges.is_empty());
        }
    }
}
