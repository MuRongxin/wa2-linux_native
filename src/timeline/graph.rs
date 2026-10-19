//! Scenario graph: which scenario follows which, which scenes are branch
//! points or endings, where sub-scripts fold into their entry, and how the
//! columns are laid out.
//!
//! The engine's jump rule: a CATCH/CATCH2/CATCH3 page in the .txt pool names
//! the next script, and the bytecode's ACT0 performs the jump with that token.
//! Sub-scripts (1008_020) carry the continuation for their entry, so their
//! jumps must be folded into the entry node - otherwise a chain like
//! 1008 -> 1009 -> ... -> 1013 disappears from the chart.

use super::*;
use crate::vm::lscr::{Op, Program};
use std::collections::{HashMap, HashSet};
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
    let shorts: Vec<&'static str> = nodes.iter().map(|n| n.chapter.short).collect();
    for i in 0..nodes.len() {
        let id = nodes[i].id.clone();
        let Some(list) = edges.get(&id) else { continue };
        let mut next: Vec<Edge> = list
            .iter()
            .filter_map(|to| index.get(to.as_str()).copied())
            .map(|to| Edge { to, kind: EdgeKind::Sequential, label: String::new() })
            .collect();
        if next.len() > 1 {
            for edge in &mut next {
                edge.kind = EdgeKind::Branch;
                edge.label = shorts[edge.to].to_string();
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
    // so a disconnected chain does not sit on top of 序章.
    let n = nodes.len();
    for _ in 0..n {
        let mut moved = false;
        for i in 0..n {
            let col = nodes[i].col;
            let next: Vec<usize> = nodes[i].next.iter().map(|e| e.to).collect();
            for j in next {
                if nodes[j].col < col + 1 {
                    nodes[j].col = col + 1;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
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
    uncollide(&mut nodes);
    nodes
}

/// Push a node down one column at a time until no two nodes share a cell.
fn uncollide(nodes: &mut [Node]) {
    for _ in 0..nodes.len() {
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
        let Some(j) = clash else { return };
        nodes[j].col += 1;
    }
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

/// Offline consistency report (wa2-native timeline-check). One line per
/// finding, meant for eyeballing after the script data changes.
pub(super) fn check(script_dir: &Path) -> Vec<String> {
    let nodes = build_nodes(script_dir);
    let mut out = Vec::new();
    out.push(format!("scenario dir: {}", script_dir.display()));
    out.push(format!("nodes: {}", nodes.len()));
    for chapter in chapters::CHAPTERS {
        let count = nodes.iter().filter(|n| std::ptr::eq(n.chapter, chapter)).count();
        if count > 0 {
            out.push(format!("  chapter {:<14} {:>3} nodes", chapter.title, count));
        }
    }
    let roots: Vec<&str> = nodes.iter().filter(|n| n.root).map(|n| n.id.as_str()).collect();
    out.push(format!("roots ({}): {}", roots.len(), roots.join(", ")));
    let endings: Vec<&str> = nodes.iter().filter(|n| n.ending).map(|n| n.id.as_str()).collect();
    out.push(format!("endings ({}): {}", endings.len(), endings.join(", ")));
    let mut branches = Vec::new();
    for node in &nodes {
        if node.next.len() > 1 {
            let targets: Vec<&str> = node.next.iter().map(|e| nodes[e.to].id.as_str()).collect();
            branches.push(format!("  {} -> {}", node.id, targets.join(", ")));
        }
    }
    out.push(format!("branch points ({}):", branches.len()));
    out.extend(branches);

    // Jump targets that are named by a script but have no node: the file is
    // missing, or the id is outside the chapter table.
    let known: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let mut dangling: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(script_dir) {
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".bnr") else { continue };
            if !chapters::wanted(super::entry_of(stem)) {
                continue;
            }
            let txt_path = ent.path().with_extension("txt");
            let (Ok(bnr), Ok(txt)) = (std::fs::read(ent.path()), std::fs::read(&txt_path)) else { continue };
            let Ok(prog) = Program::load(&bnr, &txt, false) else { continue };
            for target in script_jumps(&prog) {
                let to = super::entry_of(&target);
                if !known.contains(to) {
                    dangling.push(format!("  {} -> {} (no node)", stem, target));
                }
            }
        }
    }
    dangling.sort();
    dangling.dedup();
    out.push(format!("dangling jump targets ({}):", dangling.len()));
    out.extend(dangling);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
