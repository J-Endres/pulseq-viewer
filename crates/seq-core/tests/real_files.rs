use seq_core::model::{Analysis, NONE};

fn load(name: &str) -> Analysis {
    let path = format!(
        "{}/../../web/public/examples/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
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

#[test]
fn flash_repeated_nests() {
    let a = load("flash_repeated.seq");
    check_cover(&a);
    let loops: Vec<_> = a
        .loops
        .iter()
        .map(|l| (l.depth, l.count, l.blocks_per_iter))
        .collect();
    assert_eq!(loops, [(0, 200, 481), (1, 96, 5)]);
}

#[test]
fn collapse_delays() {
    let mut a = load("flash_je.seq");
    let full = a.display_duration;
    a.set_collapse_delays(true);
    check_cover(&a);
    // The 5 s initial delay shrinks to the median event block duration.
    assert!(a.display_duration < 0.01, "{}", a.display_duration);
    assert!(a.leaves[0].collapsed);
    let iters = vec![0; a.loops.len()];
    let (b, real, _) = a.hover(&iters, a.leaves[0].disp_dur * 0.5).unwrap();
    assert_eq!(b, 0);
    assert!((real - 2.5).abs() < 1e-9, "{real}");
    // Same loops, and switching back restores the layout.
    assert_eq!(a.loops.len(), 1);
    a.set_collapse_delays(false);
    assert_eq!(a.display_duration, full);
}
