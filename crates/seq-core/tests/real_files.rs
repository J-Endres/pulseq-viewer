use seq_core::model::{Analysis, NONE};

fn load(name: &str) -> Analysis {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(path).unwrap();
    Analysis::load(&source).unwrap()
}

fn describe(a: &Analysis) -> String {
    let mut out = format!(
        "{} blocks, {} leaves, {} loops\n",
        a.blocks.len(),
        a.leaves.len(),
        a.loops.len()
    );
    for l in &a.loops {
        out += &format!(
            "{}x{} ({} blocks/iter, first block {})\n",
            "  ".repeat(l.depth as usize),
            l.count,
            l.blocks_per_iter,
            l.first_block,
        );
    }
    out
}

/// Every block is reachable through exactly one choice of iterations, so
/// summing leaf blocks over all iterations must cover each block once.
fn check_cover(a: &Analysis) {
    fn total(a: &Analysis, parent: u32) -> u64 {
        let leaves = a.leaves.iter().filter(|l| l.parent == parent).count() as u64;
        let loops: u64 = a
            .loops
            .iter()
            .enumerate()
            .filter(|(_, l)| l.parent == parent)
            .map(|(i, l)| l.count as u64 * total(a, i as u32))
            .sum();
        leaves + loops
    }
    assert_eq!(total(a, NONE), a.blocks.len() as u64);
}

#[test]
fn flash() {
    let a = load("flash_je.seq");
    println!("{}", describe(&a));
    check_cover(&a);
    assert!(!a.loops.is_empty());
}

#[test]
fn grappa_acs() {
    let a = load("grappa_acs.seq");
    println!("{}", describe(&a));
    check_cover(&a);
}

#[test]
fn radial() {
    let a = load("seq_make_radial.seq");
    println!("{}", describe(&a));
    check_cover(&a);
}
