//! LSCR (`.bnr`) scenario bytecode: parser + interpreter.
//!
//! File layout (reverse-engineered from the shipped scripts):
//!   u32 magic = "LSCR"
//!   u32 header_size      (offset where the label table ends; 20 or 68...)
//!   u32 label_count
//!   (u32 label_id, u32 offset) * label_count   -- label 0 = main entry
//!   ...op stream...
//!
//! The op stream is a flat sequence of words. Decoded op forms:
//!   `6, kind`          record start (kind 30 = normal page, 0/8/16/27 = structural)
//!   `3, N`             token reference (index into the companion `.txt` token pool)
//!   `4, X`             page action (131 = display text, 144 = speaker,
//!                      172 = set background, 208 = choice option, 132 = wait, ...)
//!   `5, 3, V` / `5, 4, V`  int / float value
//!   `0, 12, T`         goto T
//!   `0, 2, A, B`       branch: option 1 -> fall through, option 2 -> A, B = merge
//!   `0, 4`             case-merge marker: when the preceding branch was taken as
//!                      option 1, jump to its B target (skip option 2's content)
//!   `0, 1`             return / script end
//!   `0, 13`            block end (no-op for playback)
//!   `1, sub` / `2, sub`  variable / runtime references (no-op for playback)
//!
//! The interpreter executes ops and raises `Event`s for the frontend to render.

use crate::codec::sjis;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Op {
    Rec { off: u32, kind: u32 },
    Tok { off: u32, n: u32 },
    Act { off: u32, id: u32 },
    Int { off: u32, v: i32 },
    Flt { off: u32, v: f32 },
    Goto { off: u32, target: u32 },
    Return { off: u32 },
    Branch { off: u32, a: u32, b: u32 },
    CaseEnd { off: u32 },
    Switch { off: u32, targets: Vec<u32> },
    BlockEnd { off: u32 },
    Var { off: u32, sub: u32 },
    Fun { off: u32, sub: u32 },
    Raw { off: u32, w: u32 },
}

pub fn op_off(op: &Op) -> u32 {
    match op {
        Op::Rec { off, .. }
        | Op::Tok { off, .. }
        | Op::Act { off, .. }
        | Op::Int { off, .. }
        | Op::Flt { off, .. }
        | Op::Goto { off, .. }
        | Op::Return { off }
        | Op::Branch { off, .. }
        | Op::CaseEnd { off }
        | Op::Switch { off, .. }
        | Op::BlockEnd { off }
        | Op::Var { off, .. }
        | Op::Fun { off, .. }
        | Op::Raw { off, .. } => *off,
    }
}

pub struct Program {
    pub ops: Vec<Op>,
    /// byte offset -> op index (jump targets land on op starts)
    pub index: HashMap<u32, usize>,
    pub labels: Vec<(u32, u32)>,
    /// the scenario token pool (comma-separated text in the companion `.txt`)
    pub tokens: Vec<String>,
    pub entry: u32,
}

fn rd(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl Program {
    pub fn load(bnr: &[u8], txt: &[u8]) -> Result<Program, String> {
        if bnr.len() < 20 || &bnr[0..4] != b"LSCR" {
            return Err("not an LSCR file".into());
        }
        let count = rd(bnr, 8) as usize;
        let mut labels = Vec::new();
        for k in 0..count {
            let b = 12 + k * 8;
            if b + 8 > bnr.len() {
                break;
            }
            labels.push((rd(bnr, b), rd(bnr, b + 4)));
        }
        let entry = labels
            .iter()
            .find(|(id, _)| *id == 0)
            .or_else(|| labels.first())
            .map(|(_, o)| *o)
            .unwrap_or(28);

        let tokens: Vec<String> = txt
            .split(|&b| b == b',')
            .map(|t| sjis::decode(t))
            .collect();

        let mut ops = Vec::new();
        let n = bnr.len();
        let mut i = entry as usize;
        while i + 4 <= n {
            let off = i as u32;
            let op = rd(bnr, i);
            match op {
                6 => {
                    if i + 8 > n {
                        break;
                    }
                    ops.push(Op::Rec { off, kind: rd(bnr, i + 4) });
                    i += 8;
                }
                5 => {
                    if i + 12 > n {
                        break;
                    }
                    let t = rd(bnr, i + 4);
                    let v = rd(bnr, i + 8);
                    if t == 4 {
                        ops.push(Op::Flt { off, v: f32::from_bits(v) });
                    } else {
                        ops.push(Op::Int { off, v: v as i32 });
                    }
                    i += 12;
                }
                3 => {
                    if i + 8 > n {
                        break;
                    }
                    ops.push(Op::Tok { off, n: rd(bnr, i + 4) });
                    i += 8;
                }
                4 => {
                    if i + 8 > n {
                        break;
                    }
                    ops.push(Op::Act { off, id: rd(bnr, i + 4) });
                    i += 8;
                }
                0 => {
                    let sub = rd(bnr, i + 4);
                    match sub {
                        12 => {
                            if i + 12 > n {
                                break;
                            }
                            ops.push(Op::Goto { off, target: rd(bnr, i + 8) });
                            i += 12;
                        }
                        2 => {
                            if i + 16 > n {
                                break;
                            }
                            ops.push(Op::Branch { off, a: rd(bnr, i + 8), b: rd(bnr, i + 12) });
                            i += 16;
                        }
                        1 => {
                            ops.push(Op::Return { off });
                            i += 8;
                        }
                        4 => {
                            ops.push(Op::CaseEnd { off });
                            i += 8;
                        }
                        13 => {
                            ops.push(Op::BlockEnd { off });
                            i += 8;
                        }
                        5 | 14 => {
                            // multi-target switch: args are offset-like words
                            let mut j = i + 8;
                            let mut targets = Vec::new();
                            while j + 4 <= n && targets.len() < 8 {
                                let v = rd(bnr, j);
                                if v > 64 && (v as usize) < n {
                                    targets.push(v);
                                    j += 4;
                                } else {
                                    break;
                                }
                            }
                            ops.push(Op::Switch { off, targets });
                            i = j;
                        }
                        _ => {
                            ops.push(Op::Raw { off, w: sub });
                            i += 8;
                        }
                    }
                }
                1 | 2 => {
                    if i + 8 > n {
                        break;
                    }
                    let sub = rd(bnr, i + 4);
                    ops.push(if op == 1 { Op::Var { off, sub } } else { Op::Fun { off, sub } });
                    i += 8;
                }
                _ => {
                    ops.push(Op::Raw { off, w: op });
                    i += 4;
                }
            }
        }

        let mut index = HashMap::new();
        for (k, o) in ops.iter().enumerate() {
            index.entry(op_off(o)).or_insert(k);
        }
        Ok(Program { ops, index, labels, tokens, entry })
    }

    pub fn token(&self, n: u32) -> Option<&str> {
        self.tokens.get(n as usize).map(|s| s.as_str())
    }
}

/// Explicit voice-bank and clip identifiers carried by ACT137 / ACT138.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceCue {
    pub scenario: u32,
    pub line: u32,
    pub char_id: u32,
}

/// Events raised by the interpreter; the frontend renders them.
#[derive(Debug, Clone)]
pub enum Event {
    /// Display a text token (speaker name optional; voice identifiers from ACT138).
    Text { text: String, speaker: Option<String>, voice: Option<VoiceCue> },
    /// Scene background change (ACT146/ACT147).
    ///
    /// The record is `K30/K23 vals=(mode, id, variant, frames, 0, 0, 0,
    /// 1280.0, 720.0)`; the image is `b{id:04}{variant:02}.tga` with a
    /// `b{id:04}*` fallback. `id == 0` or `-2` means a black screen.
    /// `frames` is the transition length at 60 fps.
    BgScene { id: i64, variant: u32, frames: u32 },
    /// Slot-based overlay graphic (ACT176): items, effect plates and layered-CG
    /// pieces. This is *not* a background change.
    Overlay { name: String, slot: u32 },
    /// Timed pause (ACT194), in frames at 60 fps.
    WaitFrames(u32),
    /// Original executable audio commands, with their raw integer arguments.
    Sound { opcode: u16, args: Vec<i32> },
    /// Colour filter over the whole screen (`sepia.AMP`, `nega.amp`, or empty
    /// to clear it). ACT172 only ever carries these - it is *not* a background
    /// op, and treating its empty token as "clear the background" wiped the
    /// background every time a scene dropped its tint.
    Tint { name: String },
    /// Play a movie (token like `mv01`).
    Movie { name: String },
    /// Load/switch a character's standing sprite. The file name is
    /// `{prefix}{series:03}{variant:03}.tga` inside `char.pak`.
    Sprite { char_id: u32, series: u32, variant: u32, slot: u32 },
    /// ACT156 `(char_id, 0, frames)`: that character leaves. `frames` is the
    /// exit length at 60 fps.
    SpriteExit { char_id: u32, frames: u32 },
    /// A character is now speaking (keep bright, dim the others).
    SpriteFocus { char_id: u32 },
    /// Present a choice; call `set_choice_result` with 1..=options.len().
    Choice { options: Vec<String> },
    /// Wait for the player to advance.
    Wait,
    /// Jump to another scenario (`CATCH` / `CATCH2` / `CATCH3`).
    Jump { target: String, kind: String },
    /// End of script.
    End,
}

pub struct Interp<'a> {
    pub p: &'a Program,
    pub pc: usize,
    tok: Option<u32>,
    ints: Vec<i32>,
    flts: Vec<f32>,
    speaker: Option<(String, u32)>,
    pending_voice: Option<VoiceCue>,
    voice_scenario: u32,
    options: Vec<String>,
    choice_result: u32,
    pending_b: Option<u32>,
    last_kind: String,
    steps: u64,
    /// current record tracking (for sprite-load detection)
    rec_kind: u32,
    rec_tok_hit: bool,
    rec_act_hit: bool,
    /// Every token referenced inside the current record, in order.
    ///
    /// ACT176 ("load graphic from <dir>") carries its arguments as
    /// (slot, name, x, y, ?, alpha, dir, ?) - the image name is the first token
    /// and is followed by an empty token and the directory token, so the single
    /// `tok` field (which keeps the *last* token) is not enough.
    rec_toks: Vec<u32>,
    /// `K30` record that selected a sprite face, waiting for the following
    /// `K16` record (ACT154/155) to say *where* it goes on screen.
    pending_sprite: Option<(u32, u32, u32)>,
}

/// Everything about an interpreter that has to survive a save/load round trip.
///
/// `Interp` also borrows the `Program`; that is reloaded from the script files
/// when a save is loaded (`load_program`), so it is deliberately not part of
/// this. Every field here is plain data, and `pc` is an index into
/// `Program::ops`, which is rebuilt identically on every load.
#[derive(Clone, Debug)]
pub struct InterpState {
    pub pc: usize,
    pub tok: Option<u32>,
    pub ints: Vec<i32>,
    pub flts: Vec<f32>,
    pub speaker: Option<(String, u32)>,
    pub pending_voice: Option<VoiceCue>,
    pub voice_scenario: u32,
    pub options: Vec<String>,
    /// The answer to the last choice. `Branch`/`Switch` fall through to option
    /// 1 whenever this is 1, so losing it would silently reroute the scene.
    pub choice_result: u32,
    /// Merge target of the branch being executed. Saved verbatim: `Branch` is
    /// written far more often than `CaseEnd` consumes it, so a stale value is
    /// the normal state, not a bug.
    pub pending_b: Option<u32>,
    pub last_kind: String,
    pub steps: u64,
    pub rec_kind: u32,
    pub rec_tok_hit: bool,
    pub rec_act_hit: bool,
    pub rec_toks: Vec<u32>,
    pub pending_sprite: Option<(u32, u32, u32)>,
}

impl<'a> Interp<'a> {
    pub fn snapshot(&self) -> InterpState {
        InterpState {
            pc: self.pc,
            tok: self.tok,
            ints: self.ints.clone(),
            flts: self.flts.clone(),
            speaker: self.speaker.clone(),
            pending_voice: self.pending_voice,
            voice_scenario: self.voice_scenario,
            options: self.options.clone(),
            choice_result: self.choice_result,
            pending_b: self.pending_b,
            last_kind: self.last_kind.clone(),
            steps: self.steps,
            rec_kind: self.rec_kind,
            rec_tok_hit: self.rec_tok_hit,
            rec_act_hit: self.rec_act_hit,
            rec_toks: self.rec_toks.clone(),
            pending_sprite: self.pending_sprite,
        }
    }

    /// Put the interpreter back where a save left it. The caller is responsible
    /// for handing back a `Program` for the same script, since the ops behind
    /// `pc` come from there.
    pub fn restore(&mut self, s: InterpState) {
        self.pc = s.pc;
        self.tok = s.tok;
        self.ints = s.ints;
        self.flts = s.flts;
        self.speaker = s.speaker;
        self.pending_voice = s.pending_voice;
        self.voice_scenario = s.voice_scenario;
        self.options = s.options;
        self.choice_result = s.choice_result;
        self.pending_b = s.pending_b;
        self.last_kind = s.last_kind;
        self.steps = s.steps;
        self.rec_kind = s.rec_kind;
        self.rec_tok_hit = s.rec_tok_hit;
        self.rec_act_hit = s.rec_act_hit;
        self.rec_toks = s.rec_toks;
        self.pending_sprite = s.pending_sprite;
    }
}

impl<'a> Interp<'a> {
    pub fn new(p: &'a Program) -> Interp<'a> {
        let start = p.index.get(&p.entry).copied().unwrap_or(0);
        Interp {
            p,
            pc: start,
            tok: None,
            ints: Vec::new(),
            flts: Vec::new(),
            speaker: None,
            pending_voice: None,
            voice_scenario: 0,
            options: Vec::new(),
            choice_result: 1,
            pending_b: None,
            last_kind: String::new(),
            steps: 0,
            rec_kind: 0,
            rec_tok_hit: false,
            rec_act_hit: false,
            rec_toks: Vec::new(),
            pending_sprite: None,
        }
    }

    fn goto_off(&mut self, target: u32) -> bool {
        if let Some(k) = self.p.index.get(&target) {
            self.pc = *k;
            true
        } else {
            // Land between ops: advance to the next op after the target.
            let mut best: Option<usize> = None;
            for (k, o) in self.p.ops.iter().enumerate() {
                if op_off(o) >= target {
                    best = Some(k);
                    break;
                }
            }
            match best {
                Some(k) => {
                    self.pc = k;
                    true
                }
                None => false,
            }
        }
    }

    /// Run until an event needs frontend attention.
    pub fn next_event(&mut self) -> Event {
        loop {
            if self.pc >= self.p.ops.len() {
                return Event::End;
            }
            self.steps += 1;
            if self.steps > 20_000_000 {
                return Event::End;
            }
            let op = self.p.ops[self.pc].clone();
            self.pc += 1;
            match op {
                Op::Rec { kind, .. } => {
                    // Finalize the record we just left: a bare `(char, variant,
                    // series*1000)` record followed by a `K16 ACT154/155` record
                    // is a character sprite load.
                    let sprite_ev = self.finish_record();
                    self.ints.clear();
                    self.flts.clear();
                    // Tokens are per-record: a record without a `3, N` op must not
                    // reuse the previous record's token (e.g. K23 `4, 144` = clear).
                    self.tok = None;
                    self.rec_kind = kind;
                    self.rec_tok_hit = false;
                    self.rec_act_hit = false;
                    self.rec_toks.clear();
                    if let Some(ev) = sprite_ev {
                        return ev;
                    }
                }
                Op::Tok { n, .. } => {
                    self.tok = Some(n);
                    self.rec_tok_hit = true;
                    self.rec_toks.push(n);
                }
                Op::Int { v, .. } => self.ints.push(v),
                Op::Flt { v, .. } => self.flts.push(v),
                Op::Goto { target, .. } => {
                    if !self.goto_off(target) {
                        return Event::End;
                    }
                }
                Op::Branch { a, b, .. } => {
                    self.pending_b = Some(b);
                    if self.choice_result >= 2 && self.goto_off(a) {
                        // option 2 continues at A
                    } else if self.choice_result >= 2 {
                        return Event::End;
                    }
                    // option 1 (or state branch): fall through to the case body
                }
                Op::CaseEnd { .. } => {
                    // Merge point: option 1's content band jumps to B here to skip
                    // option 2's band. Option 2 already continued at A, so this is a
                    // no-op for it. Reset the branch state for the next construct.
                    if self.choice_result < 2 {
                        if let Some(b) = self.pending_b.take() {
                            if !self.goto_off(b) {
                                return Event::End;
                            }
                        }
                    } else {
                        self.pending_b = None;
                    }
                    self.choice_result = 1;
                }
                Op::Switch { targets, .. } => {
                    if !targets.is_empty() {
                        let idx = self.choice_result.saturating_sub(1) as usize;
                        let t = targets.get(idx).copied().unwrap_or(targets[0]);
                        if !self.goto_off(t) {
                            return Event::End;
                        }
                    }
                }
                Op::Act { id, .. } => {
                    self.rec_act_hit = true;
                    if let Some(ev) = self.act(id) {
                        return ev;
                    }
                }
                Op::Fun { sub, .. } => {
                    // `2, 0` after the option records = present the choice and wait.
                    if sub == 0 {
                        if let Some(options) = self.take_options() {
                            return Event::Choice { options };
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Detect a character sprite-load record: `K30` carrying exactly three ints
    /// `(char_id, variant, series * 1000)` with no token/action, followed by a
    /// `K16` record with `ACT154`/`ACT155`. The file resolved by the frontend is
    /// `{prefix}{series:03}{variant:03}.tga` inside `char.pak`.
    fn finish_record(&mut self) -> Option<Event> {
        if self.rec_kind != 30 || self.rec_tok_hit || self.rec_act_hit || self.ints.len() != 3 {
            return None;
        }
        let cid = self.ints[0];
        let variant = self.ints[1];
        let flag = self.ints[2];
        if !((0..40).contains(&cid)
            && (0..1000).contains(&variant)
            && flag % 1000 == 0
            && (1000..=30000).contains(&flag))
        {
            return None;
        }
        let mut j = self.pc;
        let mut steps = 0;
        let mut ok = false;
        while j < self.p.ops.len() && steps < 10 {
            match &self.p.ops[j] {
                Op::Act { id, .. } => match *id {
                    154 | 155 => {
                        ok = true;
                        break;
                    }
                    131 | 132 | 144 => break,
                    _ => {}
                },
                Op::Tok { .. } => break,
                _ => {}
            }
            j += 1;
            steps += 1;
        }
        if !ok {
            return None;
        }
        // Hold it: the K16 record that follows carries the screen slot, and the
        // sprite must be placed there (slots move around during a scene).
        self.pending_sprite = Some(((cid as u32), (flag / 1000) as u32, variant as u32));
        None
    }

    fn act(&mut self, id: u32) -> Option<Event> {
        let tok = self.tok;
        match id {
            131 => {
                // display the current token as dialogue/narration
                let n = tok?;
                let text = self.p.token(n)?.to_string();
                let speaker = self.speaker.as_ref().map(|(name, _)| name.clone());
                let voice = self.pending_voice.take();
                self.speaker = None;
                Some(Event::Text { text, speaker, voice })
            }
            172 => {
                // colour filter (sepia / nega / none), never a background image
                let name = tok.and_then(|n| self.p.token(n)).unwrap_or("").to_string();
                Some(Event::Tint { name })
            }
            193 => {
                // play movie
                let name = tok.and_then(|n| self.p.token(n)).unwrap_or("").to_string();
                if name.is_empty() {
                    return None;
                }
                Some(Event::Movie { name })
            }
            144 => {
                // speaker name plate: with a name token sets it, without one it
                // clears (name plate hidden for the following narration).
                match tok {
                    Some(n) => {
                        if let Some(name) = self.p.token(n) {
                            if !name.is_empty() {
                                self.speaker = Some((name.to_string(), n));
                                self.pending_voice = None;
                            }
                        }
                    }
                    None => {
                        self.speaker = None;
                        self.pending_voice = None;
                    }
                }
                None
            }
            137 => {
                // Select the scenario's voice bank, also used by sub-scripts.
                if self.ints.len() >= 2 && self.ints[1] >= 0 {
                    self.voice_scenario = self.ints[1] as u32;
                }
                None
            }
            138 => {
                // character focus for the current line (dim the others) +
                // voice parameters for the following display
                if self.ints.len() >= 5 {
                    let cid = self.ints[self.ints.len() - 5];
                    let line = *self.ints.last().unwrap();
                    self.pending_voice = if cid >= 0 && line >= 0 {
                        Some(VoiceCue { scenario: self.voice_scenario, line: line as u32, char_id: cid as u32 })
                    } else {
                        None
                    };
                    if (0..40).contains(&cid) {
                        return Some(Event::SpriteFocus { char_id: cid as u32 });
                    }
                }
                None
            }
            208 => {
                // choice option text
                if let Some(n) = tok {
                    if let Some(t) = self.p.token(n) {
                        self.options.push(t.to_string());
                    }
                }
                None
            }
            146 | 147 => {
                // Scene background: (mode, id, variant, frames, 0, 0, 0, w, h).
                // 146 and 147 differ only in transition style.
                if self.ints.len() >= 4 {
                    let id = self.ints[1] as i64;
                    let variant = self.ints[2].max(0) as u32;
                    let frames = self.ints[3].max(0) as u32;
                    return Some(Event::BgScene { id, variant, frames });
                }
                None
            }
            176 => {
                // Slot-based overlay graphic: (slot, ?, 2, 256, flag) with
                // tokens [name, "", dir]. Items (a guitar), effect plates and
                // the layers of a composite CG come through here - it is NOT a
                // background change.
                if let Some(n) = self.rec_toks.first().and_then(|t| self.p.token(*t)) {
                    let slot = self.ints.first().copied().unwrap_or(0).max(0) as u32;
                    return Some(Event::Overlay { name: n.to_string(), slot });
                }
                None
            }
            154 | 155 => {
                // Sprite on screen; the K16 record's first value is the slot.
                // 155 is the entrance `(slot, 0, 256, 128)`, 154 the in-place
                // expression change `(slot, 0, 0, frames, 256, 128)`. Both
                // leave the character standing in that slot.
                if let Some((char_id, series, variant)) = self.pending_sprite.take() {
                    let slot = self.ints.first().copied().unwrap_or(0).max(0) as u32;
                    return Some(Event::Sprite { char_id, series, variant, slot });
                }
                None
            }
            156 => {
                // Leave. The first int is the character id, not a slot — the
                // same id K30 used when the face was chosen. Scenes dismiss
                // someone with this and keep talking; without it they stand
                // there until a later placement overwrites their slot.
                let char_id = self.ints.first().copied().unwrap_or(-1);
                if char_id < 0 {
                    return None;
                }
                let frames = self.ints.get(2).copied().unwrap_or(0).max(0) as u32;
                Some(Event::SpriteExit { char_id: char_id as u32, frames })
            }
            194 => {
                // Timed pause: (frames, 0, 0) at 60 fps.
                if let Some(f) = self.ints.first() {
                    if *f > 0 {
                        return Some(Event::WaitFrames(*f as u32));
                    }
                }
                None
            }
            158..=169 => Some(Event::Sound { opcode: id as u16, args: self.ints.clone() }),
            132 => Some(Event::Wait),
            1 => {
                // CATCH / CATCH2 / CATCH3 marker
                if let Some(t) = tok.and_then(|n| self.p.token(n)) {
                    if t.starts_with("CATCH") {
                        self.last_kind = t.to_string();
                    }
                }
                None
            }
            0 => {
                // jump to the target script id
                if let Some(t) = tok.and_then(|n| self.p.token(n)) {
                    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '_') {
                        return Some(Event::Jump {
                            target: t.to_string(),
                            kind: self.last_kind.clone(),
                        });
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Present the pending choice options and take the player's answer.
    pub fn take_options(&mut self) -> Option<Vec<String>> {
        if self.options.len() >= 2 {
            Some(std::mem::take(&mut self.options))
        } else {
            self.options.clear();
            None
        }
    }

    pub fn set_choice_result(&mut self, r: u32) {
        self.choice_result = r.max(1);
    }
}
