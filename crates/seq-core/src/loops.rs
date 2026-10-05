//! Loop detection on a token string (see docs/DESIGN.md, "Loop detection").
//!
//! The input is one token per block (its interned signature). Repeats are
//! found and collapsed into loop tokens until none remain; loop bodies are
//! compressed recursively, so nesting comes out of the same procedure.

use std::collections::{BTreeMap, HashMap};

/// Longest loop body (in tokens) the repeat search considers.
pub const P_MAX: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TokenDef {
    /// A single block with the given signature id.
    Leaf(u32),
    /// `count` consecutive copies of `body`.
    Loop { body: Vec<u32>, count: u32 },
}

/// Interned tokens: signatures first, then loops as they are found.
pub struct Grammar {
    defs: Vec<TokenDef>,
    lookup: HashMap<TokenDef, u32>,
    /// Number of blocks a token expands to.
    blocks: Vec<u64>,
    /// Whether the first block a token expands to contains an RF event.
    rf_first: Vec<bool>,
    /// Compressed bodies, keyed by the uncompressed body.
    body_memo: HashMap<Vec<u32>, Vec<u32>>,
}

impl Grammar {
    /// `leaf_rf[i]` tells whether signature `i` contains an RF event; leaf
    /// token `i` represents signature `i`.
    pub fn new(leaf_rf: &[bool]) -> Self {
        let mut g = Self {
            defs: Vec::new(),
            lookup: HashMap::new(),
            blocks: Vec::new(),
            rf_first: Vec::new(),
            body_memo: HashMap::new(),
        };
        for (i, &rf) in leaf_rf.iter().enumerate() {
            g.intern(TokenDef::Leaf(i as u32), 1, rf);
        }
        g
    }

    pub fn def(&self, token: u32) -> &TokenDef {
        &self.defs[token as usize]
    }

    pub fn blocks(&self, token: u32) -> u64 {
        self.blocks[token as usize]
    }

    fn intern(&mut self, def: TokenDef, blocks: u64, rf_first: bool) -> u32 {
        if let Some(&t) = self.lookup.get(&def) {
            return t;
        }
        let t = self.defs.len() as u32;
        self.lookup.insert(def.clone(), t);
        self.defs.push(def);
        self.blocks.push(blocks);
        self.rf_first.push(rf_first);
        t
    }

    fn intern_loop(&mut self, body: Vec<u32>, count: u32) -> u32 {
        let per_iter: u64 = body.iter().map(|&t| self.blocks(t)).sum();
        let rf_first = body.first().is_some_and(|&t| self.rf_first[t as usize]);
        self.intern(
            TokenDef::Loop { body, count },
            per_iter * count as u64,
            rf_first,
        )
    }

    /// Collapse all repeats in `s`, returning the compressed token string.
    pub fn compress(&mut self, mut s: Vec<u32>) -> Vec<u32> {
        loop {
            let selected = select(candidates(&s, &self.rf_first));
            if selected.is_empty() {
                return s;
            }
            let mut out = Vec::with_capacity(s.len());
            let mut pos = 0;
            for c in selected {
                out.extend_from_slice(&s[pos..c.start]);
                let body = self.compress_body(&s[c.start..c.start + c.period]);
                out.push(self.intern_loop(body, c.count as u32));
                pos = c.start + c.period * c.count;
            }
            out.extend_from_slice(&s[pos..]);
            s = out;
        }
    }

    fn compress_body(&mut self, body: &[u32]) -> Vec<u32> {
        if let Some(b) = self.body_memo.get(body) {
            return b.clone();
        }
        let compressed = self.compress(body.to_vec());
        self.body_memo.insert(body.to_vec(), compressed.clone());
        compressed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Candidate {
    start: usize,
    period: usize,
    count: usize,
}

impl Candidate {
    fn coverage(&self) -> usize {
        self.period * self.count
    }
    fn end(&self) -> usize {
        self.start + self.coverage()
    }
}

/// All maximal runs of `k ≥ 2` copies for every period up to `P_MAX`. Of the
/// body rotations that keep the most copies, the one starting closest before
/// an RF block (or on it) is used.
fn candidates(s: &[u32], rf_first: &[bool]) -> Vec<Candidate> {
    let n = s.len();
    let mut out = Vec::new();
    for p in 1..=P_MAX.min(n / 2) {
        let mut i = 0;
        while i + p < n {
            if s[i] != s[i + p] {
                i += 1;
                continue;
            }
            let a = i;
            while i + p < n && s[i] == s[i + p] {
                i += 1;
            }
            // s[a .. a + run] is p-periodic
            let run = i - a + p;
            if run < 2 * p {
                continue;
            }
            // Offsets 0..=run % p keep all `run / p` copies; of those, take
            // the one closest before an RF block (or on it).
            let rf_at = |r: usize| rf_first[s[a + r % p] as usize];
            let to_rf = |r: usize| (0..p).find(|&d| rf_at(r + d));
            let r = (0..=run % p)
                .min_by_key(|&r| to_rf(r).unwrap_or(0))
                .unwrap();
            out.push(Candidate {
                start: a + r,
                period: p,
                count: (run - r) / p,
            });
        }
    }
    out
}

/// Greedily pick non-overlapping candidates: most blocks covered first, then
/// smaller period, then earlier start. Returned sorted by start.
fn select(mut cands: Vec<Candidate>) -> Vec<Candidate> {
    cands.sort_by(|a, b| {
        b.coverage()
            .cmp(&a.coverage())
            .then(a.period.cmp(&b.period))
            .then(a.start.cmp(&b.start))
    });
    // Selected candidates keyed by start; they never overlap.
    let mut taken: BTreeMap<usize, Candidate> = BTreeMap::new();
    for c in cands {
        let overlaps_prev = taken
            .range(..c.end())
            .next_back()
            .is_some_and(|(_, prev)| prev.end() > c.start);
        if !overlaps_prev {
            taken.insert(c.start, c);
        }
    }
    taken.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Leaves are letters; uppercase letters contain RF.
    fn tokens(s: &str) -> (Grammar, Vec<u32>) {
        let alphabet: Vec<char> = ('a'..='z').chain('A'..='Z').collect();
        let rf: Vec<bool> = alphabet.iter().map(|c| c.is_uppercase()).collect();
        let g = Grammar::new(&rf);
        let s = s
            .chars()
            .map(|c| alphabet.iter().position(|&a| a == c).unwrap() as u32)
            .collect();
        (g, s)
    }

    fn show(g: &Grammar, s: &[u32]) -> String {
        let alphabet: Vec<char> = ('a'..='z').chain('A'..='Z').collect();
        s.iter()
            .map(|&t| match g.def(t) {
                TokenDef::Leaf(i) => alphabet[*i as usize].to_string(),
                TokenDef::Loop { body, count } => format!("({}){count}", show(g, body)),
            })
            .collect()
    }

    fn roll(s: &str) -> String {
        let (mut g, s) = tokens(s);
        let c = g.compress(s);
        show(&g, &c)
    }

    #[test]
    fn no_repeat() {
        assert_eq!(roll("abcd"), "abcd");
    }

    #[test]
    fn simple_loop() {
        assert_eq!(roll("Abc".repeat(5).as_str()), "(Abc)5");
    }

    #[test]
    fn prefix_and_suffix_stay_linear() {
        assert_eq!(roll(&format!("xy{}z", "Abc".repeat(4))), "xy(Abc)4z");
    }

    #[test]
    fn rotation_starts_at_rf() {
        // The run is found starting at `c` (equal to the trailing block), but
        // the body is rotated to start at the RF block `A`.
        assert_eq!(roll(&format!("c{}", "Abc".repeat(4))), "c(Abc)4");
    }

    #[test]
    fn rotation_keeps_copies() {
        // Rotating to `A` would leave three copies; the body starts at the
        // block before it instead.
        assert_eq!(roll(&"pAbc".repeat(4)), "(pAbc)4");
    }

    #[test]
    fn rotation_closest_before_rf() {
        // The run is found starting at `c`, one block early; the body starts
        // at `p`, right before the RF block.
        assert_eq!(roll(&format!("c{}", "pAbc".repeat(3))), "c(pAbc)3");
    }

    #[test]
    fn nested_loops() {
        let line = "Abcd";
        let slice = format!("{}e", line.repeat(8));
        assert_eq!(roll(&slice.repeat(3)), "((Abcd)8e)3");
    }

    #[test]
    fn nested_beyond_p_max() {
        // Outer body is longer than P_MAX blocks, so it is only found after
        // the inner loops collapsed.
        let line = "Abcd";
        let slice = format!("{}e", line.repeat(200));
        assert!(slice.len() > P_MAX);
        assert_eq!(roll(&slice.repeat(3)), "((Abcd)200e)3");
    }

    #[test]
    fn identical_neighbouring_blocks_inside_body() {
        assert_eq!(roll(&"Abbc".repeat(4)), "(A(b)2c)4");
    }

    #[test]
    fn block_counts() {
        let (mut g, s) = tokens(&"Abc".repeat(5));
        let c = g.compress(s);
        assert_eq!(c.len(), 1);
        assert_eq!(g.blocks(c[0]), 15);
    }
}

#[cfg(test)]
mod perf {
    use super::*;

    /// 3D-like structure: 4 averages × 64 partitions × 256 lines of 6 blocks,
    /// with a 2-block preparation per partition: ~400k blocks.
    #[test]
    #[ignore = "timing; run with --release -- --ignored"]
    fn large_nested() {
        let mut s = Vec::new();
        for avg in 0..4 {
            s.push(10 + avg % 2);
            for _ in 0..64 {
                s.extend([7, 8]);
                for _ in 0..256 {
                    s.extend([1, 2, 3, 4, 5, 6]);
                }
            }
        }
        let mut rf = vec![false; 16];
        rf[1] = true;
        rf[7] = true;
        let mut g = Grammar::new(&rf);
        let n = s.len();
        let t = std::time::Instant::now();
        let c = g.compress(s);
        println!("{n} blocks -> {} tokens in {:?}", c.len(), t.elapsed());
        assert!(c.len() <= 4);
    }
}
