//! Scenario graph: which scenario follows which, which scenes are branch
//! points or endings, where sub-scripts fold into their entry, and how the
//! columns are laid out.
//!
//! The engine's jump rule: a CATCH/CATCH2/CATCH3 page in the .txt pool names
//! the next script, and the bytecode's ACT0 performs the jump with that token.
//! Sub-scripts (1008_020) carry the continuation for their entry, so their
//! jumps must be folded into the entry node - otherwise a chain like
//! 1008 -> 1009 -> ... -> 1013 disappears from the chart.
//!
//! Edge semantics (what paint draws and the preview badge repeats):
//!   * a sequential edge is the only continuation; its label stays empty;
//!   * a node with several exits is a branch point, and every exit is a
//!     Branch edge named by where it leads: the target chapter's short route
//!     name when the route changes (2019 -> 2301 is 小春), the target scene id
//!     when the fork stays inside the chapter (2020 -> 2025 is 2025).

use super::*;
use crate::vm::lscr::{Op, Program};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Build one node per entry script (sub-scripts fold into the entry).
pub(super) fn build_nodes(script_dir: &Path) -> Vec<Node> {
    let rd = match std::fs::read_dir(script_dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    // stem -> pictures, so a subscript's CG is not thrown away when the entry
    // file opens by restating the previous scene's background.
    let mut shots: HashMap<String, Vec<(String, Vec<Shot>)>> = HashMap::new();
    let mut ids: Vec<String> = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        let Some(stem) = name.strip_suffix(".bnr") else { continue };
        let entry = super::entry_of(stem);
        if !chapters::wanted(entry) {
            continue;
        }
        if !ids.iter().any(|id| id == entry) {
            ids.push(entry.to_string());
        }
        let txt_path = ent.path().with_extension("txt");
        let Ok(bnr) = std::fs::read(ent.path()) else { continue };
        let Ok(txt) = std::fs::read(&txt_path) else { continue };
        // Shot names and jump targets are ASCII resource tokens, so the
        // Chinese-patch glyph reading is not needed here.
        let Ok(prog) = Program::load(&bnr, &txt, false) else { continue };
        let found = scene_shots(&prog, stem);
        if !found.is_empty() {
            shots.entry(entry.to_string()).or_default().push((stem.to_string(), found));
        }
        let list = edges.entry(entry.to_string()).or_default();
        for target in script_jumps(&prog) {
            let to = super::entry_of(&target);
            if to != entry && chapters::wanted(to) && !list.iter().any(|t| t == to) {
                list.push(to.to_string());
            }
        }
    }
    ids.sort();
    // A jump target that has no file of its own still needs a node.
    for list in edges.values() {
        for to in list {
            if !ids.iter().any(|id| id == to) {
                ids.push(to.clone());
            }
        }
    }
    ids.sort();
    ids.dedup();

    let index: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
    let mut nodes: Vec<Node> = ids
        .iter()
        .map(|id| {
            let chapter = chapters::of(id);
            Node {
                id: id.clone(),
                chapter,
                col: 0,
                lane: chapter.lane,
                next: Vec::new(),
                root: false,
                ending: false,
                shot: merge_shots(id, shots.get(id)),
            }
        })
        .collect();

    // Edges need every node to exist first; a branch is any node with more
    // than one outgoing script target.
    for i in 0..nodes.len() {
        let id = nodes[i].id.clone();
        let Some(list) = edges.get(&id) else { continue };
        let mut next: Vec<Edge> = list
            .iter()
            .filter_map(|to| index.get(to.as_str()).copied())
            .map(|to| Edge { to, kind: EdgeKind::Sequential, label: String::new() })
            .collect();
        if next.len() > 1 {
            let from = nodes[i].chapter;
            for edge in &mut next {
                let to = nodes[edge.to].chapter;
                edge.kind = EdgeKind::Branch;
                // Same chapter -> the scene id is the precise label; another
                // chapter -> the route's short name is what tells them apart.
                edge.label = if std::ptr::eq(from, to) {
                    nodes[edge.to].id.clone()
                } else {
                    to.short.to_string()
                };
            }
        }
        nodes[i].next = next;
    }

    // Roots and endings come from the final graph, not from file presence: the
    // 1009..1013 chain is only reachable through 1008's sub-scripts.
    let mut has_in = vec![false; nodes.len()];
    for node in &nodes {
        for edge in &node.next {
            has_in[edge.to] = true;
        }
    }
    for (i, node) in nodes.iter_mut().enumerate() {
        node.root = !has_in[i];
        node.ending = node.next.is_empty();
    }

    // Longest path from the roots. Extras (5xxx) are placed after the story
    // so a disconnected chain does not sit on top of 序章. The two rules -
    // every edge advances a column, no two nodes share a cell - can fight each
    // other (a fork has its two targets in the same column, one gets pushed
    // right), so settle both together before and after the extras shift.
    settle_layout(&mut nodes);
    let max_story = nodes
        .iter()
        .filter(|node| !node.chapter.after_story)
        .map(|node| node.col)
        .max()
        .unwrap_or(0);
    for node in &mut nodes {
        if node.chapter.after_story {
            node.col += max_story + 2;
        }
    }
    settle_layout(&mut nodes);
    nodes
}

/// Push every edge target right of its source and every clashing node down one
/// column until neither rule moves anything.
fn settle_layout(nodes: &mut [Node]) {
    for _ in 0..nodes.len().saturating_mul(2).max(2) {
        let moved_edges = relax_columns(nodes);
        let moved_cells = uncollide(nodes);
        if !moved_edges && !moved_cells {
            return;
        }
    }
}

/// Longest-path pass: every target sits at least one column right of its
/// source. Returns true when a node moved.
fn relax_columns(nodes: &mut [Node]) -> bool {
    let mut moved = false;
    for _ in 0..nodes.len() {
        let mut pass = false;
        for i in 0..nodes.len() {
            let col = nodes[i].col;
            let next: Vec<usize> = nodes[i].next.iter().map(|e| e.to).collect();
            for j in next {
                if nodes[j].col < col + 1 {
                    nodes[j].col = col + 1;
                    moved = true;
                    pass = true;
                }
            }
        }
        if !pass {
            break;
        }
    }
    moved
}

/// Push a node down one column at a time until no two nodes share a cell.
/// Returns true when a node moved.
fn uncollide(nodes: &mut [Node]) -> bool {
    let mut moved = false;
    // Each clash pushes exactly one node right; the column cap keeps a hostile
    // graph from spinning forever, it is never reached on the shipped data.
    for _ in 0..nodes.len().saturating_mul(nodes.len()).max(1) {
        let mut clash = None;
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                if nodes[i].col == nodes[j].col && nodes[i].lane == nodes[j].lane {
                    clash = Some(if nodes[i].id > nodes[j].id { i } else { j });
                    break;
                }
            }
            if clash.is_some() {
                break;
            }
        }
        let Some(j) = clash else { return moved };
        nodes[j].col += 1;
        moved = true;
    }
    moved
}

/// Backgrounds and event CGs with a real id, in script order. id 0 and id -2
/// are the fade-out and the black plate, not a picture of the scene.
fn scene_shots(prog: &Program, source: &str) -> Vec<Shot> {
    let mut ints = Vec::new();
    let mut out: Vec<Shot> = Vec::new();
    for (op_index, op) in prog.ops.iter().enumerate() {
        match op {
            Op::Rec { .. } => ints.clear(),
            Op::Int { v, .. } => ints.push(*v),
            Op::Act { id: act @ (146 | 147 | 148), .. } => {
                if ints.len() >= 4 && ints[1] > 0 {
                    let shot = Shot {
                        cg: *act == 148,
                        id: ints[1] as i64,
                        variant: ints[2].max(0),
                        op_index,
                        source: source.to_string(),
                    };
                    let duplicate = out.iter().any(|s| s.cg == shot.cg && s.id == shot.id && s.variant == shot.variant);
                    if !duplicate {
                        out.push(shot);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Entry file first, then sub-scripts by name. Later copies of a picture drop.
fn merge_shots(entry: &str, parts: Option<&Vec<(String, Vec<Shot>)>>) -> Vec<Shot> {
    let mut parts = parts.cloned().unwrap_or_default();
    parts.sort_by(|a, b| (a.0 != entry).cmp(&(b.0 != entry)).then(a.0.cmp(&b.0)));
    let mut out = Vec::new();
    for (_, found) in parts {
        for shot in found {
            if !out.iter().any(|s: &Shot| s.cg == shot.cg && s.id == shot.id && s.variant == shot.variant) {
                out.push(shot);
            }
        }
    }
    out
}

/// Numeric scenario ids named by CATCH-style jump instructions in order.
fn script_jumps(prog: &Program) -> Vec<String> {
    let mut out = Vec::new();
    let mut tok: Option<u32> = None;
    for op in &prog.ops {
        match op {
            Op::Rec { .. } => tok = None,
            Op::Tok { n, .. } => tok = Some(*n),
            Op::Act { id: 0, .. } => {
                if let Some(n) = tok {
                    if let Some(t) = prog.tokens.get(n as usize) {
                        let t = t.trim();
                        if super::is_script_id(t) {
                            out.push(t.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Every .bnr stem under the scenario dir, sorted so reports are diffable.
fn script_files(script_dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(script_dir) else { return Vec::new() };
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|ent| {
            let name = ent.file_name().to_string_lossy().to_string();
            name.strip_suffix(".bnr").map(str::to_string)
        })
        .collect();
    out.sort();
    out
}

/// Parse one scenario pair for the offline report.
fn load_program(script_dir: &Path, stem: &str) -> Option<Program> {
    let bnr = std::fs::read(script_dir.join(format!("{stem}.bnr"))).ok()?;
    let txt = std::fs::read(script_dir.join(format!("{stem}.txt"))).ok()?;
    Program::load(&bnr, &txt, false).ok()
}

/// " 1001, 5000, 5100" or "" for an empty list, so the line itself stays tidy.
fn format_list(ids: &[&str]) -> String {
    if ids.is_empty() {
        String::new()
    } else {
        format!(" {}", ids.join(", "))
    }
}

/// Offline consistency report (wa2-native timeline-check). One line per
/// finding, every list sorted, so the output is stable and diffable after the
/// script data changes.
pub(super) fn check(script_dir: &Path) -> Vec<String> {
    let nodes = build_nodes(script_dir);
    let files = script_files(script_dir);
    let mut out = Vec::new();

    out.push(format!("scenario dir: {}", script_dir.display()));
    let entries = {
        let mut set = HashSet::new();
        for stem in &files {
            set.insert(super::entry_of(stem));
        }
        set.len()
    };
    let collected = {
        let mut set = HashSet::new();
        for stem in &files {
            let entry = super::entry_of(stem);
            if chapters::wanted(entry) {
                set.insert(entry);
            }
        }
        set.len()
    };
    out.push(format!(
        "script files: {} .bnr, {} entries, {} collected",
        files.len(),
        entries,
        collected
    ));
    out.push(format!("nodes: {}", nodes.len()));

    // Per-chapter node count and the columns/lanes the chapter occupies.
    out.push(format!("chapters ({}):", chapters::CHAPTERS.len()));
    for chapter in chapters::CHAPTERS {
        let members: Vec<&Node> = nodes.iter().filter(|n| std::ptr::eq(n.chapter, chapter)).collect();
        if members.is_empty() {
            out.push(format!("  {} ({}): no nodes", chapter.title, chapter.key));
            continue;
        }
        let cols = members.iter().map(|n| n.col);
        let lanes = members.iter().map(|n| n.lane);
        let after = if chapter.after_story { ", after story" } else { "" };
        out.push(format!(
            "  {} ({}): {} nodes, cols {}..{}, lanes {}..{}{after}",
            chapter.title,
            chapter.key,
            members.len(),
            cols.clone().min().unwrap(),
            cols.max().unwrap(),
            lanes.clone().min().unwrap(),
            lanes.max().unwrap(),
        ));
    }

    // Legal entry points and dead ends.
    let mut roots: Vec<&str> = nodes.iter().filter(|n| n.root).map(|n| n.id.as_str()).collect();
    roots.sort_unstable();
    out.push(format!("roots ({}):{}", roots.len(), format_list(&roots)));
    let mut endings: Vec<&str> = nodes.iter().filter(|n| n.ending).map(|n| n.id.as_str()).collect();
    endings.sort_unstable();
    out.push(format!("endings ({}):{}", endings.len(), format_list(&endings)));

    // Branch points with their exits; a route exit is shown as "id (route)".
    let mut branches: Vec<(String, String)> = Vec::new();
    for node in &nodes {
        if node.next.len() < 2 {
            continue;
        }
        let mut targets: Vec<(&str, &str)> =
            node.next.iter().map(|e| (nodes[e.to].id.as_str(), e.label.as_str())).collect();
        targets.sort_by(|a, b| a.0.cmp(b.0));
        let shown: Vec<String> = targets
            .iter()
            .map(|(id, label)| {
                if label.is_empty() || *label == *id {
                    (*id).to_string()
                } else {
                    format!("{id} ({label})")
                }
            })
            .collect();
        branches.push((node.id.clone(), shown.join(", ")));
    }
    branches.sort_by(|a, b| a.0.cmp(&b.0));
    out.push(format!("branch points ({}):", branches.len()));
    out.extend(branches.iter().map(|(from, targets)| format!("  {from} -> {targets}")));

    // Jump targets that are named by a collected script but have no node: the
    // file is missing, or the id is outside the chapter table.
    let known: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let mut dangling: Vec<String> = Vec::new();
    for stem in &files {
        if !chapters::wanted(super::entry_of(stem)) {
            continue;
        }
        let Some(prog) = load_program(script_dir, stem) else { continue };
        for target in script_jumps(&prog) {
            let to = super::entry_of(&target);
            if !known.contains(to) {
                dangling.push(format!("  {stem} -> {target} (no node)"));
            }
        }
    }
    dangling.sort();
    dangling.dedup();
    out.push(format!("dangling jump targets ({}):", dangling.len()));
    out.extend(dangling);

    // .bnr files that exist but no chapter owns them (debug / catch / replay).
    let mut uncollected: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for stem in &files {
        let entry = super::entry_of(stem);
        if chapters::wanted(entry) {
            continue;
        }
        uncollected.entry(entry.to_string()).or_default().push(format!("{stem}.bnr"));
    }
    out.push(format!("uncollected .bnr entries ({}):", uncollected.len()));
    for (entry, mut list) in uncollected {
        list.sort();
        out.push(format!("  {entry}: {}", list.join(", ")));
    }

    // Sub-scripts folded into their entry: the chart must not show them as
    // separate nodes, but the fold still has to cover every file.
    let mut folded: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for stem in &files {
        let entry = super::entry_of(stem);
        if stem == entry || !chapters::wanted(entry) {
            continue;
        }
        folded.entry(entry.to_string()).or_default().push(stem.clone());
    }
    let folded_total: usize = folded.values().map(Vec::len).sum();
    out.push(format!("collapsed sub-scripts ({folded_total} in {} entries):", folded.len()));
    for (entry, mut list) in folded {
        list.sort();
        out.push(format!("  {entry}: {} sub-scripts ({})", list.len(), list.join(", ")));
    }

    // A node whose file is missing was only reachable as a jump target.
    let mut orphans: Vec<&str> = nodes
        .iter()
        .filter(|n| !files.iter().any(|stem| super::entry_of(stem) == n.id))
        .map(|n| n.id.as_str())
        .collect();
    orphans.sort_unstable();
    out.push(format!("nodes without a .bnr file ({}):{}", orphans.len(), format_list(&orphans)));

    // Nodes no root can reach.
    let mut seen = vec![false; nodes.len()];
    let mut stack: Vec<usize> =
        nodes.iter().enumerate().filter(|(_, n)| n.root).map(|(i, _)| i).collect();
    while let Some(i) = stack.pop() {
        if seen[i] {
            continue;
        }
        seen[i] = true;
        for edge in &nodes[i].next {
            stack.push(edge.to);
        }
    }
    let mut lost: Vec<&str> = nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| !seen[*i])
        .map(|(_, n)| n.id.as_str())
        .collect();
    lost.sort_unstable();
    out.push(format!("unreachable nodes ({}):{}", lost.len(), format_list(&lost)));

    // Layout sanity: one node per (column, lane) cell and a forward-only graph.
    let mut cells: HashSet<(i32, i32)> = HashSet::new();
    let mut clashes: Vec<&str> = Vec::new();
    for node in &nodes {
        if !cells.insert((node.col, node.lane)) {
            clashes.push(node.id.as_str());
        }
    }
    clashes.sort_unstable();
    let col = nodes.iter().map(|n| n.col);
    let lane = nodes.iter().map(|n| n.lane);
    let (min_col, max_col) = (col.clone().min().unwrap_or(0), col.max().unwrap_or(0));
    let (min_lane, max_lane) = (lane.clone().min().unwrap_or(0), lane.max().unwrap_or(0));
    out.push(format!(
        "layout: {} cell overlaps, cols {min_col}..{max_col}, lanes {min_lane}..{max_lane}",
        clashes.len()
    ));
    if !clashes.is_empty() {
        out.push(format!("  overlapping nodes: {}", clashes.join(", ")));
    }
    let mut backward: Vec<String> = nodes
        .iter()
        .flat_map(|n| n.next.iter().map(move |e| (n, e)))
        .filter(|(n, e)| nodes[e.to].col <= n.col)
        .map(|(n, e)| format!("  {} (col {}) -> {} (col {})", n.id, n.col, nodes[e.to].id, nodes[e.to].col))
        .collect();
    backward.sort();
    out.push(format!("edges that do not advance a column ({}):", backward.len()));
    out.extend(backward);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart() -> Vec<Node> {
        build_nodes(Path::new("data/script"))
    }

    fn at(nodes: &[Node], id: &str) -> usize {
        nodes.iter().position(|n| n.id == id).unwrap_or_else(|| panic!("missing node {id}"))
    }

    fn next(nodes: &[Node], id: &str) -> Vec<String> {
        nodes[at(nodes, id)].next.iter().map(|e| nodes[e.to].id.clone()).collect()
    }

    fn sorted_next(nodes: &[Node], id: &str) -> Vec<String> {
        let mut v = next(nodes, id);
        v.sort();
        v
    }

    fn label(nodes: &[Node], from: &str, to: &str) -> String {
        nodes[at(nodes, from)]
            .next
            .iter()
            .find(|e| nodes[e.to].id == to)
            .unwrap_or_else(|| panic!("no edge {from} -> {to}"))
            .label
            .clone()
    }

    /// Freeze the graph shape the chart and the report are built on.
    #[test]
    fn the_graph_shape_is_frozen() {
        let nodes = chart();
        assert_eq!(nodes.len(), 163);
        assert_eq!(nodes.iter().filter(|n| n.next.len() > 1).count(), 9, "branch points");
        assert_eq!(nodes.iter().filter(|n| n.ending).count(), 11, "endings");
        let counts = [
            ("ic", 13),
            ("cc_common", 19),
            ("cc_setsuna", 14),
            ("cc_koharu", 22),
            ("cc_chiaki", 13),
            ("cc_mari", 17),
            ("coda", 24),
            ("coda_kazusa", 11),
            ("coda_setsuna", 11),
            ("coda_extra", 9),
            ("novel_before", 5),
            ("novel_after", 5),
        ];
        for (key, want) in counts {
            let chapter = chapters::CHAPTERS.iter().find(|c| c.key == key).unwrap();
            let got = nodes.iter().filter(|n| std::ptr::eq(n.chapter, chapter)).count();
            assert_eq!(got, want, "chapter {key}");
        }
        // One node per entry, sub-scripts folded in.
        let mut ids = std::collections::HashSet::new();
        for node in &nodes {
            assert!(ids.insert(node.id.as_str()), "duplicate node {}", node.id);
            assert!(!node.id.contains('_'), "sub-script {} became a node", node.id);
        }
    }

    /// Roots are legal entries (no incoming edge), endings are dead ends.
    #[test]
    fn roots_and_endings_are_exact() {
        let nodes = chart();
        let mut has_in = vec![false; nodes.len()];
        for node in &nodes {
            for edge in &node.next {
                has_in[edge.to] = true;
            }
        }
        for (i, node) in nodes.iter().enumerate() {
            assert_eq!(node.root, !has_in[i], "{} root flag", node.id);
            assert_eq!(node.ending, node.next.is_empty(), "{} ending flag", node.id);
        }
        let mut roots: Vec<&str> = nodes.iter().filter(|n| n.root).map(|n| n.id.as_str()).collect();
        roots.sort_unstable();
        assert_eq!(roots, vec!["1001", "5000", "5100"]);
        let mut endings: Vec<&str> = nodes.iter().filter(|n| n.ending).map(|n| n.id.as_str()).collect();
        endings.sort_unstable();
        assert_eq!(
            endings,
            vec!["2025", "2322", "2412", "2413", "2517", "3024", "3111", "3211", "3909", "5004", "5104"]
        );
        // 2025 is a real CC-Setsuna ending node, not a phantom target: four
        // scenes fork to it while 2024 carries the main line on.
        let i = at(&nodes, "2025");
        assert!(nodes[i].ending && !nodes[i].root);
        assert_eq!(nodes[i].chapter.key, "cc_setsuna");
        for from in ["2020", "2021", "2022", "2023"] {
            assert!(next(&nodes, from).contains(&"2025".to_string()), "{from} should fork to 2025");
        }
        assert_eq!(sorted_next(&nodes, "2024"), vec!["2026"]);
        assert_eq!(sorted_next(&nodes, "2033"), vec!["3001"], "CC 雪菜 main line leads into CODA");
    }

    /// Branch edges are labelled by target route, or by target scene inside
    /// the same chapter; sequential edges stay unlabelled.
    #[test]
    fn branch_edges_are_labelled_by_route_or_scene() {
        let nodes = chart();
        // 2019 is the four-way choice: four Branch edges, one per route.
        let choices = &nodes[at(&nodes, "2019")].next;
        assert_eq!(choices.len(), 4);
        assert!(choices.iter().all(|e| e.kind == EdgeKind::Branch));
        assert_eq!(sorted_next(&nodes, "2019"), vec!["2020", "2301", "2401", "2501"]);
        assert_eq!(label(&nodes, "2019", "2020"), "CC 雪菜");
        assert_eq!(label(&nodes, "2019", "2301"), "小春");
        assert_eq!(label(&nodes, "2019", "2401"), "千晶");
        assert_eq!(label(&nodes, "2019", "2501"), "麻理");
        // Same-chapter forks name the target scene.
        assert_eq!(sorted_next(&nodes, "2020"), vec!["2021", "2025"]);
        assert_eq!(label(&nodes, "2020", "2021"), "2021");
        assert_eq!(label(&nodes, "2020", "2025"), "2025");
        assert_eq!(sorted_next(&nodes, "2023"), vec!["2024", "2025"]);
        // The CODA split: 雪菜 route and 和纱 route left/right of the table.
        assert_eq!(sorted_next(&nodes, "3023"), vec!["3101", "3201"]);
        assert_eq!(label(&nodes, "3023", "3101"), "和纱");
        assert_eq!(label(&nodes, "3023", "3201"), "雪菜");
        // A branch into another chapter uses its short name; a same-chapter
        // branch keeps the scene id (CODA-EXTRA is its own chapter).
        assert_eq!(sorted_next(&nodes, "3016"), vec!["3017", "3024", "3901"]);
        assert_eq!(label(&nodes, "3016", "3017"), "3017");
        assert_eq!(label(&nodes, "3016", "3024"), "3024");
        assert_eq!(label(&nodes, "3016", "3901"), "EXTRA");
        // Every edge kind/label is consistent with the fan-out.
        for node in &nodes {
            if node.next.len() > 1 {
                for edge in &node.next {
                    assert_eq!(edge.kind, EdgeKind::Branch, "{} fork edge", node.id);
                    assert!(!edge.label.is_empty(), "{} -> {} has no label", node.id, nodes[edge.to].id);
                    assert!(chapters::wanted(&nodes[edge.to].id), "branch to an unowned scenario");
                }
            } else if let Some(edge) = node.next.first() {
                assert_eq!(edge.kind, EdgeKind::Sequential, "{} singular edge", node.id);
                assert!(edge.label.is_empty(), "{} -> {} carries a route label", node.id, nodes[edge.to].id);
            }
        }
        assert_eq!(label(&nodes, "1008", "1009"), "");
    }

    /// Columns follow the script order, extras sit after the story and no two
    /// nodes share a chart cell.
    #[test]
    fn the_layout_advances_columns_without_overlapping_cells() {
        let nodes = chart();
        let mut cells = std::collections::HashSet::new();
        for node in &nodes {
            assert!(
                cells.insert((node.col, node.lane)),
                "{} overlaps another node at col {}, lane {}",
                node.id,
                node.col,
                node.lane
            );
            assert_eq!(node.lane, node.chapter.lane, "{} lane", node.id);
        }
        for node in &nodes {
            for edge in &node.next {
                assert!(
                    nodes[edge.to].col > node.col,
                    "{} -> {} does not advance a column",
                    node.id,
                    nodes[edge.to].id
                );
            }
        }
        // IC -> CC -> CODA order, and the CC choice happens before its branches.
        assert!(nodes[at(&nodes, "1001")].col < nodes[at(&nodes, "1013")].col);
        assert!(nodes[at(&nodes, "1013")].col < nodes[at(&nodes, "2001")].col);
        assert!(nodes[at(&nodes, "2001")].col < nodes[at(&nodes, "2019")].col);
        for id in ["2020", "2301", "2401", "2501"] {
            assert!(nodes[at(&nodes, "2019")].col < nodes[at(&nodes, id)].col);
        }
        // After-story novels are pushed to the right of the whole story.
        let max_story = nodes
            .iter()
            .filter(|n| !n.chapter.after_story)
            .map(|n| n.col)
            .max()
            .unwrap();
        for node in nodes.iter().filter(|n| n.chapter.after_story) {
            assert!(node.col > max_story, "{} is not after the story", node.id);
        }
        // One band per chapter on the chart, describing exactly its nodes.
        let bands = chapters::bands(&nodes);
        assert_eq!(bands.len(), chapters::CHAPTERS.len());
        for band in &bands {
            let members: Vec<&Node> = nodes.iter().filter(|n| std::ptr::eq(n.chapter, band.chapter)).collect();
            assert!(!members.is_empty(), "empty band {}", band.chapter.key);
            assert_eq!(band.first_col, members.iter().map(|n| n.col).min().unwrap());
            assert_eq!(band.last_col, members.iter().map(|n| n.col).max().unwrap());
            assert_eq!(band.min_lane, members.iter().map(|n| n.lane).min().unwrap());
            assert_eq!(band.max_lane, members.iter().map(|n| n.lane).max().unwrap());
        }
        // Chapters that share a lane must occupy disjoint column segments
        // (CODA-雪菜 sits on CC-麻理's purple row, the novels on lanes 0/3 -
        // all after the story, so no two bands of the same row can overlap).
        for (i, a) in bands.iter().enumerate() {
            for b in &bands[i + 1..] {
                if a.chapter.lane != b.chapter.lane {
                    continue;
                }
                assert!(
                    a.last_col < b.first_col || b.last_col < a.first_col,
                    "{} and {} share lane {} and overlap in columns ({}..{} / {}..{})",
                    a.chapter.key,
                    b.chapter.key,
                    a.chapter.lane,
                    a.first_col,
                    a.last_col,
                    b.first_col,
                    b.last_col
                );
            }
        }
    }

    #[test]
    fn common_route_reaches_four_branches_without_subscripts() {
        let nodes = build_nodes(Path::new("data/script"));
        assert!(nodes.len() > 40, "expected the chapter scripts, got {}", nodes.len());
        assert!(nodes.iter().all(|n| !n.id.contains('_')));
        let at = |id: &str| nodes.iter().position(|n| n.id == id).unwrap();
        let next = |id: &str| -> Vec<&str> {
            nodes[at(id)].next.iter().map(|e| nodes[e.to].id.as_str()).collect()
        };
        assert!(nodes[at("1001")].col < nodes[at("1013")].col);
        assert!(nodes[at("1013")].col < nodes[at("2001")].col);
        assert!(nodes[at("2001")].col < nodes[at("2019")].col);
        let targets = next("2019");
        for id in ["2020", "2301", "2401", "2501"] {
            assert!(targets.contains(&id), "2019 should reach {id}, got {targets:?}");
            assert!(nodes[at("2019")].col < nodes[at(id)].col);
        }
        assert_eq!(nodes[at("2019")].next.len(), 4);
        assert!(nodes[at("2019")].next.iter().all(|e| e.kind == EdgeKind::Branch));
        // No two scenes occupy the same cell.
        let mut seen = std::collections::HashSet::new();
        for n in &nodes {
            assert!(seen.insert((n.col, n.lane)), "overlap {} at {},{}", n.id, n.col, n.lane);
        }
        let pictured = nodes.iter().filter(|n| !n.shot.is_empty()).count();
        assert!(pictured * 10 > nodes.len() * 8, "most scenes should name a picture, {pictured}/{}", nodes.len());
        assert!(!nodes[at("2301")].shot.is_empty());
        assert!(!nodes[at("3001")].shot.is_empty());
    }

    #[test]
    fn the_ic_chain_continues_through_its_subscripts() {
        // 1008/1009/... only name the next entry from inside their _020 pages.
        let nodes = build_nodes(Path::new("data/script"));
        let at = |id: &str| nodes.iter().position(|n| n.id == id).unwrap();
        let next = |id: &str| -> Vec<&str> {
            nodes[at(id)].next.iter().map(|e| nodes[e.to].id.as_str()).collect()
        };
        assert_eq!(next("1008"), vec!["1009"]);
        assert_eq!(next("1009"), vec!["1010"]);
        assert_eq!(next("1012"), vec!["1013"]);
        assert!(!nodes[at("1009")].root, "1009 is reached through 1008");
        assert!(!nodes[at("1011")].root);
    }

    #[test]
    fn every_node_is_reachable_from_a_root() {
        let nodes = build_nodes(Path::new("data/script"));
        let mut seen = vec![false; nodes.len()];
        let mut stack: Vec<usize> = nodes.iter().enumerate().filter(|(_, n)| n.root).map(|(i, _)| i).collect();
        while let Some(i) = stack.pop() {
            if seen[i] {
                continue;
            }
            seen[i] = true;
            for edge in &nodes[i].next {
                stack.push(edge.to);
            }
        }
        let lost: Vec<&str> = nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| !seen[*i])
            .map(|(_, n)| n.id.as_str())
            .collect();
        assert!(lost.is_empty(), "nodes are not reachable from any root: {lost:?}");
    }

    #[test]
    fn endings_are_the_dead_ends() {
        let nodes = build_nodes(Path::new("data/script"));
        for node in &nodes {
            assert_eq!(node.ending, node.next.is_empty(), "{}", node.id);
        }
        let ending = |id: &str| nodes.iter().find(|n| n.id == id).map(|n| n.ending).unwrap_or(false);
        assert!(ending("3111") && ending("3211") && ending("3909"));
        assert!(!ending("1001"));
    }

    /// timeline-check must stay readable, sorted and complete enough to verify
    /// a script-data change by eye.
    #[test]
    fn check_report_lists_the_whole_graph_and_is_stable() {
        let dir = Path::new("data/script");
        let report = check(dir);
        assert_eq!(report, check(dir), "check output must be deterministic");
        let has = |needle: &str| report.iter().any(|line| line == needle);
        let starts = |needle: &str| report.iter().any(|line| line.starts_with(needle));
        assert!(has("script files: 187 .bnr, 168 entries, 163 collected"));
        assert!(has("nodes: 163"));
        assert!(report.iter().any(|l| l.contains("(ic): 13 nodes, cols 0..")));
        assert!(report.iter().any(|l| l.contains("(cc_setsuna): 14 nodes")));
        assert!(has("roots (3): 1001, 5000, 5100"));
        assert!(starts("endings (11): 2025, 2322, 2412"));
        assert!(has("branch points (9):"));
        assert!(has("  2019 -> 2020 (CC 雪菜), 2301 (小春), 2401 (千晶), 2501 (麻理)"));
        assert!(has("  2020 -> 2021, 2025"));
        assert!(has("  3016 -> 3017, 3024, 3901 (EXTRA)"));
        assert!(has("dangling jump targets (0):"));
        assert!(has("uncollected .bnr entries (5):"));
        for debug in ["9999", "LF3", "catch", "catch2", "catch3"] {
            assert!(starts(&format!("  {debug}: ")), "missing uncollected entry {debug}");
        }
        assert!(has("collapsed sub-scripts (19 in 6 entries):"));
        assert!(has("  1008: 4 sub-scripts (1008_020, 1008_030, 1008_040, 1008_050)"));
        assert!(has("  1012: 3 sub-scripts (1012_020, 1012_030, 1012_030_2)"));
        assert!(has("  3014: 2 sub-scripts (3014_2, 3014_3)"));
        assert!(has("nodes without a .bnr file (0):"));
        assert!(has("unreachable nodes (0):"));
        assert!(starts("layout: 0 cell overlaps"));
        assert!(has("edges that do not advance a column (0):"));
    }
}
