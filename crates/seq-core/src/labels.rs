//! Label (`LABELSET` / `LABELINC`) operations per block and the running
//! label values, taken from the parsed sequence.

use pulseq_rs::seq;

/// All label names in display order: counters, then flags.
const ORDER: [&str; 23] = [
    "SLC", "SEG", "REP", "AVG", "SET", "ECO", "PHS", "LIN", "PAR", "ACQ", "TRID", "NAV", "REV",
    "SMS", "REF", "IMA", "OFF", "NOISE", "PMC", "NOROT", "NOPOS", "NOSCL", "ONCE",
];

pub struct Labels {
    /// Labels the sequence uses, in `ORDER`.
    pub names: Vec<String>,
    /// Blocks with label operations, ascending.
    blocks: Vec<usize>,
    /// Per such block: its operations, e.g. `LIN=48 AVG+1`.
    texts: Vec<String>,
    /// Per such block: values of `names` after its operations.
    values: Vec<Vec<i32>>,
}

enum Op {
    Set(String, i32),
    Inc(String, i32),
}

impl Labels {
    pub fn from_seq(seq: &seq::Sequence) -> Self {
        let ops: Vec<Vec<Op>> = seq
            .blocks
            .iter()
            .map(|b| {
                // Sets before increments, as the interpreter applies them.
                let sets = b.ext.iter().filter_map(|e| match e {
                    seq::Extension::LabelSet { flag, value } => {
                        Some(Op::Set(flag.to_string(), *value))
                    }
                    _ => None,
                });
                let incs = b.ext.iter().filter_map(|e| match e {
                    seq::Extension::LabelInc { counter, value } => {
                        Some(Op::Inc(counter.to_string(), *value))
                    }
                    _ => None,
                });
                let mut ops: Vec<Op> = sets.chain(incs).collect();
                // Same text in every iteration, whatever order the file uses.
                ops.sort_by_key(|op| match op {
                    Op::Set(n, _) => (0, rank(n)),
                    Op::Inc(n, _) => (1, rank(n)),
                });
                ops
            })
            .collect();

        let names: Vec<String> = ORDER
            .iter()
            .filter(|n| {
                ops.iter().flatten().any(|op| match op {
                    Op::Set(m, _) | Op::Inc(m, _) => m == *n,
                })
            })
            .map(|n| n.to_string())
            .collect();
        let index = |name: &str| names.iter().position(|n| n == name);

        let mut current = vec![0; names.len()];
        let mut out = Self {
            names: Vec::new(),
            blocks: Vec::new(),
            texts: Vec::new(),
            values: Vec::new(),
        };
        for (b, block_ops) in ops.iter().enumerate() {
            if block_ops.is_empty() {
                continue;
            }
            // Operations that change a value come first in the text, so a
            // truncated marker still shows them.
            let mut changes = Vec::new();
            let mut repeats = Vec::new();
            for op in block_ops {
                let (name, text) = match op {
                    Op::Set(name, v) => (name, format!("{name}={v}")),
                    Op::Inc(name, v) => (name, format!("{name}{v:+}")),
                };
                let changed = index(name).is_some_and(|i| {
                    let old = current[i];
                    current[i] = match op {
                        Op::Set(_, v) => *v,
                        Op::Inc(_, v) => old.wrapping_add(*v),
                    };
                    current[i] != old
                });
                if changed {
                    changes.push(text);
                } else {
                    repeats.push(text);
                }
            }
            changes.append(&mut repeats);
            out.blocks.push(b);
            out.texts.push(changes.join(" "));
            out.values.push(current.clone());
        }
        out.names = names;
        out
    }

    /// Label values after block `block`'s operations.
    pub fn at(&self, block: usize) -> Vec<i32> {
        let i = self.blocks.partition_point(|&b| b <= block);
        match i.checked_sub(1) {
            Some(i) => self.values[i].clone(),
            None => vec![0; self.names.len()],
        }
    }

    /// Index of `block`'s operations, if it has any.
    pub fn event(&self, block: usize) -> Option<usize> {
        self.blocks.binary_search(&block).ok()
    }

    pub fn text(&self, event: usize) -> &str {
        &self.texts[event]
    }
}

fn rank(name: &str) -> usize {
    ORDER.iter().position(|n| *n == name).unwrap_or(ORDER.len())
}

#[cfg(test)]
mod tests {
    #[test]
    fn order_is_complete() {
        let mut names = super::ORDER.to_vec();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), super::ORDER.len());
    }
}
