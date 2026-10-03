//! Loaded sequence, its loop tree laid out in display time, and the queries
//! the frontend needs (see docs/DESIGN.md).

use std::collections::HashMap;
use std::sync::Arc;

use num_complex::Complex64;

use pulseq_rs::{int, seq};

use crate::labels::Labels;
use crate::loops::{Grammar, TokenDef};

/// Larmor frequency used for the interpreted sequence `[Hz]` (1H at 3 T).
pub const LARMOR: f64 = 42.577_478_518e6 * 3.0;

/// Plotted channels, in row order.
pub const RF_MAG: usize = 0;
pub const RF_PHASE: usize = 1;
pub const GX: usize = 2;
pub const GY: usize = 3;
pub const GZ: usize = 4;
pub const ADC: usize = 5;
/// Receiver phase; drawn in the RF phase row.
pub const ADC_PHASE: usize = 6;
pub const CHANNELS: usize = 7;

/// Marks "no parent loop".
pub const NONE: u32 = u32::MAX;

/// Signature bits for the event channels a block uses.
const SIG_RF: u8 = 1 << 0;
const SIG_GX: u8 = 1 << 1;
const SIG_GY: u8 = 1 << 2;
const SIG_GZ: u8 = 1 << 3;
const SIG_ADC: u8 = 1 << 4;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Signature {
    /// Block duration in block-raster ticks.
    pub ticks: u64,
    /// `SIG_*` bits.
    pub channels: u8,
}

impl Signature {
    pub fn of(block: &int::Block, block_raster: f64) -> Self {
        let mut channels = 0;
        if block.rf.is_some() {
            channels |= SIG_RF;
        }
        if block.gx.is_some() {
            channels |= SIG_GX;
        }
        if block.gy.is_some() {
            channels |= SIG_GY;
        }
        if block.gz.is_some() {
            channels |= SIG_GZ;
        }
        if block.adc.is_some() {
            channels |= SIG_ADC;
        }
        Self {
            ticks: (block.duration / block_raster).round() as u64,
            channels,
        }
    }

    pub fn has_rf(&self) -> bool {
        self.channels & SIG_RF != 0
    }
}

/// A loop instance in the display layout. Loops are stored in pre-order, so a
/// parent always comes before its children.
#[derive(Clone, Debug)]
pub struct LoopNode {
    pub parent: u32,
    pub depth: u32,
    pub count: u32,
    pub blocks_per_iter: u64,
    /// Block offset from the start of the parent's current iteration (or from
    /// block 0 at the top level).
    pub offset: u64,
    /// First block of iteration 0 when all enclosing loops are at iteration 0.
    pub first_block: u64,
    pub disp_start: f64,
    pub disp_dur: f64,
}

/// A block position in the display layout.
#[derive(Clone, Copy, Debug)]
pub struct Leaf {
    pub parent: u32,
    pub offset: u64,
    pub disp_start: f64,
    pub disp_dur: f64,
    /// Drawn shorter than its real duration (a collapsed delay block).
    pub collapsed: bool,
}

pub struct Analysis {
    pub name: Option<String>,
    pub warnings: Vec<String>,
    pub blocks: Vec<int::Block>,
    /// Real start time of each block, plus the total duration at the end.
    pub block_start: Vec<f64>,
    pub loops: Vec<LoopNode>,
    pub leaves: Vec<Leaf>,
    pub display_duration: f64,
    /// Max |value| per channel over the whole sequence, in display units.
    pub channel_max: [f64; CHANNELS],
    /// Per block and channel: (min, max) in display units, NaN if absent.
    summary: Vec<[[f32; 2]; CHANNELS]>,
    pub labels: Labels,
    grammar: Grammar,
    /// Compressed top-level token string.
    top: Vec<u32>,
    /// Display duration of collapsed delay blocks: the median duration of
    /// blocks with events.
    delay_cap: f64,
    collapse_delays: bool,
}

impl Analysis {
    pub fn load(source: &str) -> Result<Self, String> {
        let parsed = seq::Sequence::from_source(source).map_err(|e| e.to_string())?;
        let block_raster = parsed.time_raster.block;
        let (seq, warnings) =
            int::Sequence::from_seq(&parsed, int::Transform::default(), LARMOR, HashMap::new())
                .map_err(|e| e.to_string())?;
        let warnings = warnings.iter().map(|w| w.to_string()).collect();
        let labels = Labels::from_seq(&parsed);
        drop(parsed);

        let blocks = seq.blocks;
        let mut block_start = Vec::with_capacity(blocks.len() + 1);
        let mut t = 0.0;
        for b in &blocks {
            block_start.push(t);
            t += b.duration;
        }
        block_start.push(t);

        // Signatures → tokens
        let mut sig_ids: HashMap<Signature, u32> = HashMap::new();
        let mut leaf_rf = Vec::new();
        let tokens: Vec<u32> = blocks
            .iter()
            .map(|b| {
                let sig = Signature::of(b, block_raster);
                *sig_ids.entry(sig).or_insert_with(|| {
                    leaf_rf.push(sig.has_rf());
                    leaf_rf.len() as u32 - 1
                })
            })
            .collect();

        let mut grammar = Grammar::new(&leaf_rf);
        let top = grammar.compress(tokens);

        let mut event_durations: Vec<f64> = blocks
            .iter()
            .filter(|b| !is_delay(b))
            .map(|b| b.duration)
            .collect();
        event_durations.sort_by(f64::total_cmp);
        let delay_cap = event_durations
            .get(event_durations.len() / 2)
            .copied()
            .unwrap_or(f64::INFINITY);

        let mut stats = ShapeStats::default();
        let summary: Vec<_> = blocks.iter().map(|b| stats.block(b)).collect();
        let mut channel_max = [0.0f64; CHANNELS];
        for s in &summary {
            for ch in 0..CHANNELS {
                let [lo, hi] = s[ch];
                if !lo.is_nan() {
                    channel_max[ch] = channel_max[ch]
                        .max((lo as f64).abs())
                        .max((hi as f64).abs());
                }
            }
        }
        channel_max[RF_PHASE] = std::f64::consts::PI;
        channel_max[ADC] = 1.0;
        channel_max[ADC_PHASE] = std::f64::consts::PI;

        let mut a = Self {
            name: seq.name,
            warnings,
            blocks,
            block_start,
            loops: Vec::new(),
            leaves: Vec::new(),
            display_duration: 0.0,
            channel_max,
            summary,
            labels,
            grammar,
            top,
            delay_cap,
            collapse_delays: false,
        };
        a.layout();
        Ok(a)
    }

    /// Draw delay blocks (no events) at most `delay_cap` wide.
    pub fn set_collapse_delays(&mut self, collapse: bool) {
        if collapse != self.collapse_delays {
            self.collapse_delays = collapse;
            self.layout();
        }
    }

    fn layout(&mut self) {
        let mut layout = Layout {
            grammar: &self.grammar,
            blocks: &self.blocks,
            delay_cap: self.collapse_delays.then_some(self.delay_cap),
            loops: Vec::new(),
            leaves: Vec::with_capacity(self.top.len()),
        };
        let (_, display_duration) = layout.sequence(&self.top, NONE, 0, 0, 0, 0.0);
        self.loops = layout.loops;
        self.leaves = layout.leaves;
        self.display_duration = display_duration;
    }

    /// Start block of the current iteration of every loop, for the given
    /// iteration indices (one per loop, clamped to the loop's count).
    fn iteration_starts(&self, iters: &[u32]) -> Vec<u64> {
        let mut starts = Vec::with_capacity(self.loops.len());
        for (i, l) in self.loops.iter().enumerate() {
            let base = if l.parent == NONE {
                0
            } else {
                starts[l.parent as usize]
            };
            let it = iters.get(i).copied().unwrap_or(0).min(l.count - 1) as u64;
            starts.push(base + l.offset + it * l.blocks_per_iter);
        }
        starts
    }

    fn resolve(&self, starts: &[u64], leaf: &Leaf) -> usize {
        let base = if leaf.parent == NONE {
            0
        } else {
            starts[leaf.parent as usize]
        };
        (base + leaf.offset) as usize
    }

    /// Leaves overlapping the display-time window `[t0, t1]`.
    fn visible(&self, t0: f64, t1: f64) -> &[Leaf] {
        let first = self
            .leaves
            .partition_point(|l| l.disp_start + l.disp_dur < t0);
        let last = self.leaves.partition_point(|l| l.disp_start <= t1);
        &self.leaves[first..last.max(first)]
    }

    /// Per channel and pixel column, the (min, max) of the plotted value in
    /// `[t0, t1]`; NaN where nothing is drawn. Layout: `[channel][column][2]`.
    pub fn waveforms(&self, iters: &[u32], t0: f64, t1: f64, columns: usize) -> Vec<f32> {
        let mut env = Envelope::new(t0, t1, columns);
        let starts = self.iteration_starts(iters);
        let col_width = (t1 - t0) / columns.max(1) as f64;
        for leaf in self.visible(t0, t1) {
            let b = self.resolve(&starts, leaf);
            let block = &self.blocks[b];
            if leaf.disp_dur < 2.0 * col_width {
                self.add_summary(&mut env, b, leaf.disp_start);
            } else {
                add_block(&mut env, block, leaf.disp_start);
            }
        }
        env.data
    }

    fn add_summary(&self, env: &mut Envelope, b: usize, t: f64) {
        let block = &self.blocks[b];
        let s = &self.summary[b];
        let mut range = |ch: usize, (start, end): (f64, f64)| {
            let [lo, hi] = s[ch];
            env.range(ch, t + start, t + end, lo, hi);
        };
        if let Some(rf) = &block.rf {
            let ext = (rf.delay, rf.delay + rf.shape.duration);
            range(RF_MAG, ext);
            range(RF_PHASE, ext);
        }
        for (ch, g) in [(GX, &block.gx), (GY, &block.gy), (GZ, &block.gz)] {
            if let Some(g) = g {
                range(ch, grad_extent(g));
            }
        }
        if let Some(adc) = &block.adc {
            range(ADC, adc_extent(adc));
            range(ADC_PHASE, adc_extent(adc));
        }
    }

    /// Display times of ADC samples in `[t0, t1]`, or nothing if there are
    /// more than `max`.
    pub fn adc_samples(&self, iters: &[u32], t0: f64, t1: f64, max: usize) -> Vec<f64> {
        let starts = self.iteration_starts(iters);
        let mut out = Vec::new();
        for leaf in self.visible(t0, t1) {
            let b = self.resolve(&starts, leaf);
            let Some(adc) = &self.blocks[b].adc else {
                continue;
            };
            let first = leaf.disp_start + adc.delay + 0.5 * adc.dwell;
            let i0 = (((t0 - first) / adc.dwell).ceil().max(0.0)) as u32;
            for i in i0..adc.num {
                let ts = first + i as f64 * adc.dwell;
                if ts > t1 {
                    break;
                }
                out.push(ts);
                if out.len() > max {
                    return Vec::new();
                }
            }
        }
        out
    }

    /// Block index, real time and display start of the block under display
    /// time `t`, or nothing outside the timeline.
    pub fn hover(&self, iters: &[u32], t: f64) -> Option<(usize, f64, f64)> {
        let i = self
            .leaves
            .partition_point(|l| l.disp_start + l.disp_dur <= t);
        let leaf = self.leaves.get(i).filter(|l| l.disp_start <= t)?;
        let b = self.resolve(&self.iteration_starts(iters), leaf);
        let real_dur = self.blocks[b].duration;
        let into = (t - leaf.disp_start) * real_dur / leaf.disp_dur;
        Some((b, self.block_start[b] + into, leaf.disp_start))
    }

    /// `(display start, label event)` of blocks with label operations in
    /// `[t0, t1]`, at most `max` of them.
    pub fn label_marks(&self, iters: &[u32], t0: f64, t1: f64, max: usize) -> Vec<(f64, usize)> {
        let starts = self.iteration_starts(iters);
        self.visible(t0, t1)
            .iter()
            .filter_map(|l| {
                let e = self.labels.event(self.resolve(&starts, l))?;
                Some((l.disp_start, e))
            })
            .take(max)
            .collect()
    }

    /// `(display start, display end, real duration)` of collapsed delay
    /// blocks overlapping `[t0, t1]`, at most `max` of them.
    pub fn collapsed(&self, iters: &[u32], t0: f64, t1: f64, max: usize) -> Vec<[f64; 3]> {
        let starts = self.iteration_starts(iters);
        self.visible(t0, t1)
            .iter()
            .filter(|l| l.collapsed)
            .take(max)
            .map(|l| {
                let b = self.resolve(&starts, l);
                [
                    l.disp_start,
                    l.disp_start + l.disp_dur,
                    self.blocks[b].duration,
                ]
            })
            .collect()
    }
}

/// A block without RF, gradient or ADC events.
fn is_delay(b: &int::Block) -> bool {
    b.rf.is_none() && b.gx.is_none() && b.gy.is_none() && b.gz.is_none() && b.adc.is_none()
}

/// Builds `loops` and `leaves` from the compressed token string.
struct Layout<'a> {
    grammar: &'a Grammar,
    blocks: &'a [int::Block],
    /// Maximum display duration of delay blocks, if they are collapsed.
    delay_cap: Option<f64>,
    loops: Vec<LoopNode>,
    leaves: Vec<Leaf>,
}

impl Layout<'_> {
    /// Lays out `tokens` one after another. `abs` is the absolute block index
    /// of the first token with all loops at iteration 0. Returns the number
    /// of blocks and the display duration.
    fn sequence(
        &mut self,
        tokens: &[u32],
        parent: u32,
        depth: u32,
        offset: u64,
        abs: u64,
        t: f64,
    ) -> (u64, f64) {
        let mut blocks = 0;
        let mut dur = 0.0;
        for &tok in tokens {
            let (b, d) = self.token(tok, parent, depth, offset + blocks, abs + blocks, t + dur);
            blocks += b;
            dur += d;
        }
        (blocks, dur)
    }

    /// Returns the blocks the token covers in total and its display duration.
    fn token(
        &mut self,
        tok: u32,
        parent: u32,
        depth: u32,
        offset: u64,
        abs: u64,
        t: f64,
    ) -> (u64, f64) {
        match self.grammar.def(tok) {
            TokenDef::Leaf(_) => {
                // Every iteration of the enclosing loops has the same
                // signature here, so iteration 0 stands for all of them.
                let block = &self.blocks[abs as usize];
                let disp_dur = match self.delay_cap {
                    Some(cap) if is_delay(block) => block.duration.min(cap),
                    _ => block.duration,
                };
                self.leaves.push(Leaf {
                    parent,
                    offset,
                    disp_start: t,
                    disp_dur,
                    collapsed: disp_dur < block.duration,
                });
                (1, disp_dur)
            }
            TokenDef::Loop { body, count } => {
                let id = self.loops.len() as u32;
                self.loops.push(LoopNode {
                    parent,
                    depth,
                    count: *count,
                    blocks_per_iter: 0,
                    offset,
                    first_block: abs,
                    disp_start: t,
                    disp_dur: 0.0,
                });
                let (per_iter, disp_dur) = self.sequence(body, id, depth + 1, 0, abs, t);
                let node = &mut self.loops[id as usize];
                node.blocks_per_iter = per_iter;
                node.disp_dur = disp_dur;
                debug_assert_eq!(per_iter * *count as u64, self.grammar.blocks(tok));
                (per_iter * *count as u64, disp_dur)
            }
        }
    }
}

/// Per-pixel-column min/max accumulator for all channels.
struct Envelope {
    t0: f64,
    /// Columns per second.
    scale: f64,
    columns: usize,
    data: Vec<f32>,
}

impl Envelope {
    fn new(t0: f64, t1: f64, columns: usize) -> Self {
        Self {
            t0,
            scale: columns as f64 / (t1 - t0),
            columns,
            data: vec![f32::NAN; CHANNELS * columns * 2],
        }
    }

    fn x(&self, t: f64) -> f64 {
        (t - self.t0) * self.scale
    }

    fn add(&mut self, ch: usize, col: usize, v: f32) {
        let i = (ch * self.columns + col) * 2;
        let d = &mut self.data[i..i + 2];
        if d[0].is_nan() || v < d[0] {
            d[0] = v;
        }
        if d[1].is_nan() || v > d[1] {
            d[1] = v;
        }
    }

    /// Visible column range for display x in `[xa, xb]`.
    fn columns(&self, xa: f64, xb: f64) -> Option<(usize, usize)> {
        if xb < 0.0 || xa >= self.columns as f64 {
            return None;
        }
        let ca = xa.floor().max(0.0) as usize;
        let cb = (xb.floor().max(0.0) as usize).min(self.columns - 1);
        Some((ca, cb))
    }

    /// Straight line from `(ta, va)` to `(tb, vb)`, `ta <= tb`.
    fn segment(&mut self, ch: usize, ta: f64, va: f64, tb: f64, vb: f64) {
        let (xa, xb) = (self.x(ta), self.x(tb));
        let Some((ca, cb)) = self.columns(xa, xb) else {
            return;
        };
        if xb - xa <= 0.0 {
            self.add(ch, ca, va as f32);
            self.add(ch, ca, vb as f32);
            return;
        }
        let slope = (vb - va) / (xb - xa);
        for c in ca..=cb {
            let lo = xa.max(c as f64);
            let hi = xb.min(c as f64 + 1.0);
            self.add(ch, c, (va + slope * (lo - xa)) as f32);
            self.add(ch, c, (va + slope * (hi - xa)) as f32);
        }
    }

    /// Value range `[lo, hi]` over the whole time span `[ta, tb]`.
    fn range(&mut self, ch: usize, ta: f64, tb: f64, lo: f32, hi: f32) {
        if lo.is_nan() {
            return;
        }
        let Some((ca, cb)) = self.columns(self.x(ta), self.x(tb)) else {
            return;
        };
        for c in ca..=cb {
            self.add(ch, c, lo);
            self.add(ch, c, hi);
        }
    }
}

/// Gradients are plotted in kHz/m.
const GRAD_UNIT: f64 = 1e-3;

fn grad_extent(g: &int::Gradient) -> (f64, f64) {
    let time = &g.shape.time;
    (
        g.delay + time.first().copied().unwrap_or(0.0),
        g.delay + time.last().copied().unwrap_or(0.0),
    )
}

fn adc_extent(adc: &int::Adc) -> (f64, f64) {
    (adc.delay, adc.delay + adc.num as f64 * adc.dwell)
}

/// Range covered by phases `lo..=hi` after wrapping to (-π, π].
fn wrapped_range(lo: f64, hi: f64) -> (f64, f64) {
    use std::f64::consts::PI;
    let w = wrap_phase(lo);
    if hi - lo >= 2.0 * PI || w + (hi - lo) > PI {
        (-PI, PI)
    } else {
        (w, w + (hi - lo))
    }
}

fn wrap_phase(p: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    p - TAU * ((p + PI) / TAU).floor()
}

/// Draws one block exactly, starting at display time `t`.
fn add_block(env: &mut Envelope, block: &int::Block, t: f64) {
    if let Some(rf) = &block.rf {
        let s = &rf.shape;
        let t_rf = t + rf.delay;
        let mag = |i: usize| rf.amp.abs() * s.amp[i].norm();
        for i in 1..s.time.len() {
            env.segment(
                RF_MAG,
                t_rf + s.time[i - 1],
                mag(i - 1),
                t_rf + s.time[i],
                mag(i),
            );
        }
        if s.time.len() == 1 {
            env.segment(RF_MAG, t_rf, mag(0), t_rf + s.duration, mag(0));
        }
        // Phase only where the pulse is on.
        let offset = rf_phase_offset(rf);
        let phase = |i: usize| wrap_phase(s.amp[i].arg() + offset);
        let mut prev: Option<usize> = None;
        for i in 0..s.time.len() {
            if s.amp[i].norm() == 0.0 {
                prev = None;
                continue;
            }
            match prev {
                Some(p) => env.segment(
                    RF_PHASE,
                    t_rf + s.time[p],
                    phase(p),
                    t_rf + s.time[i],
                    phase(i),
                ),
                None => env.segment(
                    RF_PHASE,
                    t_rf + s.time[i],
                    phase(i),
                    t_rf + s.time[i],
                    phase(i),
                ),
            }
            prev = Some(i);
        }
    }
    for (ch, g) in [(GX, &block.gx), (GY, &block.gy), (GZ, &block.gz)] {
        if let Some(g) = g {
            let s = &g.shape;
            let t_g = t + g.delay;
            let v = |i: usize| g.amp * s.amp[i] * GRAD_UNIT;
            if s.time.len() == 1 {
                env.segment(ch, t_g + s.time[0], v(0), t_g + s.time[0], v(0));
            }
            for i in 1..s.time.len() {
                env.segment(ch, t_g + s.time[i - 1], v(i - 1), t_g + s.time[i], v(i));
            }
        }
    }
    if let Some(adc) = &block.adc {
        let (a, b) = adc_extent(adc);
        env.segment(ADC, t + a, 1.0, t + b, 1.0);
        match &adc.phase_shape {
            None => {
                let p = wrap_phase(adc.phase);
                env.segment(ADC_PHASE, t + a, p, t + b, p);
            }
            Some(s) => {
                let t_adc = t + adc.delay;
                let p = |i: usize| wrap_phase(adc.phase + s.amp[i]);
                if s.time.len() == 1 {
                    env.segment(ADC_PHASE, t + a, p(0), t + b, p(0));
                }
                for i in 1..s.time.len() {
                    env.segment(
                        ADC_PHASE,
                        t_adc + s.time[i - 1],
                        p(i - 1),
                        t_adc + s.time[i],
                        p(i),
                    );
                }
            }
        }
    }
}

/// Per-shape statistics, cached by shape pointer (pulseq-rs shares shapes
/// between blocks through `Arc`).
#[derive(Default)]
struct ShapeStats {
    real: HashMap<usize, (f64, f64)>,
    /// (min |a|, max |a|, phase if it is the same for every nonzero sample)
    complex: HashMap<usize, (f64, f64, Option<f64>)>,
}

impl ShapeStats {
    fn real(&mut self, shape: &Arc<int::Shape<f64>>) -> (f64, f64) {
        *self
            .real
            .entry(Arc::as_ptr(shape) as usize)
            .or_insert_with(|| {
                shape
                    .amp
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &a| {
                        (lo.min(a), hi.max(a))
                    })
            })
    }

    fn complex(&mut self, shape: &Arc<int::Shape<Complex64>>) -> (f64, f64, Option<f64>) {
        *self
            .complex
            .entry(Arc::as_ptr(shape) as usize)
            .or_insert_with(|| {
                let mut lo = f64::INFINITY;
                let mut hi = f64::NEG_INFINITY;
                let mut phase: Option<Option<f64>> = None;
                for a in &shape.amp {
                    let m = a.norm();
                    lo = lo.min(m);
                    hi = hi.max(m);
                    if m > 0.0 {
                        let arg = a.arg();
                        phase = match phase {
                            None => Some(Some(arg)),
                            Some(Some(p)) if (p - arg).abs() < 1e-9 => Some(Some(p)),
                            _ => Some(None),
                        };
                    }
                }
                (lo, hi, phase.flatten())
            })
    }

    /// (min, max) per channel of one block, NaN for absent channels.
    fn block(&mut self, block: &int::Block) -> [[f32; 2]; CHANNELS] {
        use std::f64::consts::PI;
        let mut out = [[f32::NAN; 2]; CHANNELS];
        let mut put =
            |ch: usize, lo: f64, hi: f64| out[ch] = [lo.min(hi) as f32, lo.max(hi) as f32];
        if let Some(rf) = &block.rf {
            let (lo, hi, phase) = self.complex(&rf.shape);
            put(RF_MAG, rf.amp.abs() * lo, rf.amp.abs() * hi);
            match phase {
                Some(p) => {
                    let p = wrap_phase(p + rf_phase_offset(rf));
                    put(RF_PHASE, p, p);
                }
                None => put(RF_PHASE, -PI, PI),
            }
        }
        for (ch, g) in [(GX, &block.gx), (GY, &block.gy), (GZ, &block.gz)] {
            if let Some(g) = g {
                let (lo, hi) = self.real(&g.shape);
                put(ch, g.amp * lo * GRAD_UNIT, g.amp * hi * GRAD_UNIT);
            }
        }
        if let Some(adc) = &block.adc {
            put(ADC, 1.0, 1.0);
            let (lo, hi) = match &adc.phase_shape {
                Some(shape) => self.real(shape),
                None => (0.0, 0.0),
            };
            let (lo, hi) = wrapped_range(adc.phase + lo, adc.phase + hi);
            put(ADC_PHASE, lo, hi);
        }
        out
    }
}

/// Constant phase added to every RF sample: the event phase, plus π for a
/// negative amplitude (the magnitude is plotted as |amp|).
fn rf_phase_offset(rf: &int::Rf) -> f64 {
    rf.phase
        + if rf.amp < 0.0 {
            std::f64::consts::PI
        } else {
            0.0
        }
}
