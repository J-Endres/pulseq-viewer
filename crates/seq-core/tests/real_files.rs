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

#[test]
fn adc_phase_follows_adc_event() {
    use seq_core::model::{ADC_PHASE, CHANNELS};
    use std::f64::consts::{PI, TAU};
    let a = load("flash_je.seq");
    let l = &a.loops[0];
    let columns = 400;
    for iter in [0u32, 1, 7] {
        let w = a.waveforms(&[iter], l.disp_start, l.disp_start + l.disp_dur, columns);
        let ch = &w[ADC_PHASE * columns * 2..(ADC_PHASE + 1) * columns * 2];
        let drawn: Vec<f32> = ch.iter().copied().filter(|v| !v.is_nan()).collect();
        assert!(!drawn.is_empty());
        // The ADC of this iteration
        let first = (l.first_block + iter as u64 * l.blocks_per_iter) as usize;
        let adc = (first..first + l.blocks_per_iter as usize)
            .find_map(|b| a.blocks[b].adc.as_ref())
            .unwrap();
        let expected = adc.phase - TAU * ((adc.phase + PI) / TAU).floor();
        assert!(
            drawn.iter().all(|&v| (v as f64 - expected).abs() < 1e-5),
            "iter {iter}"
        );
    }
    assert_eq!(w_len(&a, columns), CHANNELS * columns * 2);
}

fn w_len(a: &Analysis, columns: usize) -> usize {
    a.waveforms(&[0], 0.0, 1.0, columns).len()
}

#[test]
fn labels_match_interpreter() {
    let a = load("flash_je.seq");
    assert_eq!(
        a.labels.names,
        ["AVG", "ECO", "LIN", "PAR", "REV", "REF", "IMA"]
    );
    let lin = a.labels.names.iter().position(|n| n == "LIN").unwrap();
    let mut adcs = 0;
    for (b, block) in a.blocks.iter().enumerate() {
        if let Some(adc) = &block.adc {
            assert_eq!(a.labels.at(b)[lin], adc.labels.lin, "block {b}");
            adcs += 1;
        }
    }
    assert_eq!(adcs, 96);
    // Label marks follow the selected iteration.
    let l = &a.loops[0];
    let text = |iter: u32| {
        a.label_marks(&[iter], l.disp_start, l.disp_start + l.disp_dur, 10)
            .iter()
            .map(|&(_, e)| a.labels.text(e).to_string())
            .collect::<Vec<_>>()
    };
    // flash_je re-sets several labels every TR; changed values come first.
    assert_eq!(text(0), ["LIN=48 AVG=0 PAR=0 REF=0 IMA=0", "ECO=0 REV=0"]);
    assert_eq!(text(1), ["LIN=47 AVG=0 PAR=0 REF=0 IMA=0", "ECO=0 REV=0"]);
}

#[test]
fn hover_over_stacked_loops() {
    let a = load("flash_repeated.seq");
    let (outer, inner) = (&a.loops[0], &a.loops[1]);
    // Middle of the readout block of the inner loop body
    let leaf = a
        .leaves
        .iter()
        .find(|l| {
            l.parent == 1
                && a.blocks[(inner.first_block + l.offset) as usize]
                    .adc
                    .is_some()
        })
        .unwrap();
    let t = leaf.disp_start + leaf.disp_dur / 2.0;
    let iters = [0, 0];
    assert_eq!(a.hover_blocks(&iters, &[0, 0], t).len(), 1);
    assert_eq!(
        a.hover_blocks(&iters, &[0, 1], t).len(),
        inner.count as usize
    );
    assert_eq!(
        a.hover_blocks(&iters, &[1, 0], t).len(),
        outer.count as usize
    );
    assert_eq!(
        a.hover_blocks(&iters, &[1, 1], t).len(),
        (outer.count * inner.count) as usize
    );
    // Stacking the line loop covers every LIN value once.
    let lin = a.labels.names.iter().position(|n| n == "LIN").unwrap();
    let mut lins: Vec<i32> = a
        .hover_blocks(&iters, &[0, 1], t)
        .iter()
        .map(|&b| a.labels.at(b)[lin])
        .collect();
    lins.sort();
    assert_eq!(lins, (0..96).collect::<Vec<_>>());
}

#[test]
fn time_segments_follow_real_time() {
    let mut a = load("flash_je.seq");
    a.set_collapse_delays(true);
    let end = a.display_duration;
    let delay = &a.leaves[0];
    let mid = delay.disp_dur / 2.0;
    // Iteration 0: the collapsed delay is cut at its centre; afterwards real
    // time runs without further jumps through the whole loop.
    let s = a.time_segments(&[0], 0.0, end);
    assert_eq!(s.len(), 2, "{s:?}");
    assert_eq!((s[0].0, s[0].2), (0.0, 0.0));
    assert!((s[0].1 - mid).abs() < 1e-12);
    // Second half ends exactly where block 1 starts.
    assert!((s[1].2 + (delay.disp_dur - mid) - a.block_start[1]).abs() < 1e-9);
    // Iteration 5: an extra jump at the loop start.
    let l = &a.loops[0];
    let s = a.time_segments(&[5], 0.0, end);
    let piece = s
        .iter()
        .find(|p| (p.0 - l.disp_start).abs() < 1e-12)
        .unwrap();
    let first = (l.first_block + 5 * l.blocks_per_iter) as usize;
    assert!((piece.2 - a.block_start[first]).abs() < 1e-12);
}

/// Multi-shot TSE: a dummy shot without ADC, then 4 shots of 16 echoes. The
/// last block of the dummy shot equals the last block of every shot, so the
/// shot repeat is found one block early and must not lose an iteration.
#[test]
fn tse_shots() {
    let a = load("tse.seq");
    check_cover(&a);
    let loops: Vec<_> = a
        .loops
        .iter()
        .map(|l| (l.depth, l.count, l.blocks_per_iter, l.first_block))
        .collect();
    assert_eq!(loops, [(0, 16, 4, 3), (0, 4, 70, 70), (1, 16, 4, 73)]);
}
