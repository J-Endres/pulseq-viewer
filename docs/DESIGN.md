# Pulseq Viewer — Design

A static web page, hosted on GitHub Pages, that opens a Pulseq `.seq` file and
plots it. Its distinguishing feature is that loops unrolled in the file (phase
encoding, slices, averages, …) are detected and shown rolled up: the timeline
runs left to right, and each detected loop occupies the width of a single
iteration, with controls to step through its iterations or overlay all of them.

Everything runs in the browser. The file is read locally and never uploaded.

## Architecture

```
            ┌──────────────── browser ────────────────┐
 .seq file →│ web/ (TypeScript)                        │
            │   file input → text ─┐                   │
            │                      ▼                   │
            │   crates/seq-core (Rust → wasm)          │
            │     parse (pulseq-rs)                    │
            │     block signatures → loop tree         │
            │     waveform extraction                  │
            │                      │ typed arrays      │
            │                      ▼                   │
            │   timeline model → canvas renderer       │
            │   loop controls (single / stacked)       │
            └──────────────────────────────────────────┘
```

- **`crates/seq-core`** — Rust library compiled to wasm with `wasm-bindgen`.
  Owns parsing, loop detection and waveform extraction. Unit-tested natively
  with `cargo test` against real `.seq` files.
- **`web/`** — Vite + TypeScript, no UI framework. Owns the file input,
  the timeline model, the canvas renderer and the loop controls.
- The boundary between the two is coarse: a handful of calls that return flat
  typed arrays (`Float64Array`, `Uint32Array`, …), never per-block JS objects.

### Repository layout

```
crates/seq-core/        Rust crate
  src/loops.rs          loop detection on token strings
  src/labels.rs         label operations and running label values
  src/model.rs          loading, signatures, display layout, waveforms
  src/lib.rs            wasm API
  tests/                tests against the example .seq files
web/                    Vite app
  src/main.ts           entry point, file loading, pointer/keyboard input
  src/state.ts          loops, iteration indices, view window, change events
  src/render.ts         canvas renderer
  src/controls.ts       loop panel
  src/examples.ts       built-in example list
  src/theme.ts          colour theme menu
  src/colormap.ts       viridis colour map for stacked iterations
  public/examples/      example .seq files, served with the site
scripts/                generator for the long example sequence
  src/wasm/             wasm-bindgen output (generated, not committed)
docs/DESIGN.md          this document
.github/workflows/      Pages build + deploy
```

## Loading

1. The user picks a file (file input or drag and drop onto the page), or one
   of the built-in examples from the header's example menu.
2. The page reads the file as text — an example is fetched from
   `./examples/<name>.seq` only when it is picked — and passes it to wasm.
   When loads overlap, only the most recent one is shown.
3. `seq-core` loads the file with
   [pulseq-rs](https://github.com/pulseq-frame/pulseq-rs), pinned as a git
   dependency to a fixed revision: `seq::Sequence::from_source(&str)` parses
   it, and `int::Sequence::from_seq(..)` turns it into the interpreted
   sequence (FOV scaling, rotations and soft delays applied). It is built with
   an identity transform, a 3 T Larmor frequency and an empty soft-delay map.

   The interpreted sequence is the single model the viewer works with: loop
   detection and plotting both read its blocks.
4. Parse or interpretation errors are shown on the page with their message.
   `console_error_panic_hook` turns Rust panics into readable console errors.

## Loop detection

### Block signature

Each block of the interpreted sequence is reduced to a signature of its
duration and which event channels it uses:

| field    | value                                          |
|----------|------------------------------------------------|
| duration | block duration in block-raster ticks (integer) |
| RF       | block has an RF event                          |
| Gx       | block has a gradient on x                      |
| Gy       | block has a gradient on y                      |
| Gz       | block has a gradient on z                      |
| ADC      | block has an ADC event                         |

Amplitudes, phases, frequencies, shapes, delays within the block, labels and
triggers are not part of the signature. A block with no events (a pure delay)
has a signature of just its duration.

Durations are converted to integer ticks (`round(duration / block_raster)`,
with the block raster from the file's definitions) so comparisons are exact.

Signatures are interned: each distinct signature gets a small integer token,
and the sequence becomes a token string `s` of length `n` (number of blocks).

### Finding repeats

A *repeat* is a substring `s[a .. a + p·k]` made of `k ≥ 2` consecutive copies
of a body of length `p`. For every period `p` from 1 to `P_max`:

1. Scan once, tracking the length `L` of the current run where
   `s[i] == s[i + p]` holds.
2. A run starting at `a` with length `L` is a repeat with body length `p` and
   `k = ⌊L / p⌋ + 1` copies.

`P_max` is 512 tokens. Cost is `O(n · P_max)` per pass.

### Choosing where an iteration starts

A repeat spanning `m = L + p` blocks can be read with its body starting at
any of `p` offsets. Offsets `0 ..= m mod p` keep all `⌊m / p⌋` copies; the
others lose one. Of the offsets that keep all copies, the one closest before
an RF block is used: an RF block itself if there is one, otherwise the offset
with the fewest blocks before the body's first RF block (e.g. a slice-select
ramp-up block). The blocks before that offset stay outside the loop. If the
body has no RF block, the run starts where the scan found it.

Never giving up a copy matters when the block before the first iteration
happens to equal the last block of every iteration (e.g. a dummy shot ending
with the same rewinder as the imaging shots): the run is then found one
block early, and rotating it onto the RF block would cut off the last
iteration.

### Collapsing and nesting

Each pass collects the repeats of all periods and selects a non-overlapping
set greedily: most blocks covered (`p·k`) first, then smaller period, then
earlier start. Every selected repeat is replaced in `s` by a single token
representing `loop(body, k)`, and the next pass runs on the shortened string,
until no repeat with `k ≥ 2` remains.

The body of a selected repeat is compressed with the same procedure before
the loop token is made, so loops nested inside it are found too. Loop tokens
are interned like signatures: two loops are the same token exactly when they
have the same compressed body and the same `k`.

Nesting therefore comes out in two ways that give the same tree: an outer loop
whose body fits in `P_max` is found first and its inner loops come from
compressing its body; an outer loop with a longer body is found on a later
pass, after its inner loops have collapsed into single tokens.

### Result: the loop tree

The final string is the top level of a tree:

```
Node = Block
     | Loop { body: [Node], count: k }
```

Because every iteration of a loop consists of identical tokens, every
iteration spans the same number of blocks `m` and the same duration. The
concrete block for a position in the body is therefore pure arithmetic on the
current iteration indices.

## Timeline model

The display axis is **display time**, built from the loop tree:

- A top-level block occupies its real duration.
- A loop occupies the duration of **one** iteration of its body, with nested
  loops inside the body also counted as one iteration.

The tree is laid out once after loading, in pre-order, into two lists:

- **Loops**: parent loop, depth, count `k`, blocks per iteration `m`, block
  offset from the start of the parent's iteration, display start and display
  duration.
- **Leaves** (block positions): parent loop, block offset from the start of
  the parent's iteration, display start and display duration. Leaves are
  sorted by display start, so the blocks in a time window are found by binary
  search.

For given iteration indices, the first block of a loop's current iteration is
`start(parent) + offset + iteration · m`, computed for all loops in one pass
over the pre-ordered list; a leaf's block is `start(parent) + offset`. Real
time at a display time is the real start of the resolved block plus the offset
into it.

### Collapsed delays

A *delay block* is a block without RF, gradient or ADC events. With
**Collapse delays** on (a header toggle, on by default and remembered per
browser), each delay block is laid out with a display duration of at most the
median duration of the blocks that have events; shorter delays keep their
real duration. Loop detection is unaffected (signatures still use the real
duration), and since all iterations of a loop share their signatures, a
collapsed delay inside a loop has the same width in every iteration.
Toggling re-runs the layout in wasm and keeps the current iterations.

Inside a collapsed block, display time maps linearly onto the block's real
duration, so the hover readout still shows real time. The renderer shades
collapsed blocks across all rows, marks their edges with dashed lines and
labels them with their real duration when it fits.

## Labels

`labels.rs` reads the `LABELSET` / `LABELINC` operations of every block from
the parsed sequence (the interpreted one keeps only per-ADC snapshots) and
applies them like the interpreter: sets before increments, in block order.
It keeps:

- the labels the sequence uses, in a fixed order (counters SLC … TRID, then
  flags NAV … ONCE);
- for every block with label operations: the operations as text, sorted by
  that order with value-changing operations first (`LIN=48 AVG=0 …`), and the
  values of all labels after them.

The values at any block come from a binary search over those blocks.

- **Label row**: a tick at every visible block with label operations, for
  the current iterations, with its text where there is room before the next
  tick (truncated with "…").
- **Hover panel**: while the pointer is over the plot, the side panel shows
  the label values after the hovered block (or all their values under stacked
  loops, see below) instead of the loop controls,
  with values the block changed highlighted; it switches back when the
  pointer leaves the plot. Sequences without labels keep the loop panel.

## Iterations: single and stacked

Every loop is shown in one of two modes, chosen per loop:

- **Single** (default): one iteration, selected with the loop's slider.
- **Stacked**: all iterations overlaid, colour-coded by iteration index.

- **State**: per loop an iteration index (starting at 0) and a stacked
  flag. A loop nested in another loop's body has one shared index,
  independent of which outer iteration is shown.
- **Controls**:
  - Above the plot, every loop has a bracket spanning its segment, one row per
    nesting depth, labelled with its name and current iteration
    (`Loop 1.2 · 7 / 128`). Loops are named by position: top-level loops
    `1`, `2`, …; loops inside loop `1` are `1.1`, `1.2`, ….
  - Scrolling the mouse wheel over a bracket steps that loop's iteration;
    clicking the bracket focuses it, after which arrow keys step it (Home and
    End jump to the first and last iteration, Escape clears the focus).
    Stepping does nothing on a stacked loop.
  - A side panel lists all loops, nested by depth, each with its count,
    blocks per iteration, real duration of one iteration, a **Stack** toggle,
    and a slider and number field for its iteration. While stacked, the
    slider is replaced by a colour bar from 1 to the loop's count. The
    focused loop is highlighted in both the panel and the plot.
  - A stacked loop's bracket is drawn with the colour scale and labelled
    `all 96`, or `24 of 96` when not all iterations are drawn (below).
- **Fixed y-scales**: each channel's y-range is the maximum over the whole
  sequence, so stepping or overlaying iterations shows amplitude changes
  instead of rescaling.

### Overlay

Every draw first splits the visible window into intervals, each owned by the
innermost stacked loop covering it, or by no stacked loop.

- Outside stacked intervals, the current iterations are drawn once in the
  channel colours, as in single mode.
- Inside them, the renderer draws one waveform set per combination of the
  iterations of the visible stacked loops, each from its own `waveforms`
  call over the span of the stacked intervals. In each interval, a
  combination is drawn in the colour of the owning loop's iteration (viridis,
  restricted per theme to the part that stays visible on its background) and
  clipped to that interval. ADC phase is dashed there, since the channel
  colours no longer tell RF from ADC.
- At most 256 combinations are drawn: the loop with the most iterations to
  draw is halved (evenly spread iterations, first and last included) until
  the product fits.
- ADC windows do not change between iterations and are drawn once.
- In the label row, ticks inside stacked intervals have no text, because
  the text would describe a single iteration.
- Runs of identical flat columns are drawn as one line segment, which keeps
  the overlay of ~100 iterations at about 30 ms per frame.

### Hover with stacked loops

Under a stacked loop, the pointer covers one block per iteration.
`hover_blocks` returns the blocks under the pointer for every combination of
the iterations of the stacked loops enclosing it, with all of them, not just
the drawn ones. The footer then shows how many blocks are overlaid and their
range, and the label panel lists each label's unique values over those
blocks: up to six values as a list, otherwise as ranges of consecutive values
(`0–95`) when there are at most four ranges, otherwise as `min–max (n
values)`. Labels with more than one value are highlighted.

## Rendering

- **One `<canvas>`** filling the plot area, sized with `devicePixelRatio` for
  sharp lines. Rows from top to bottom: RF magnitude (with ADC), phase (RF
  and ADC), Gx, Gy, Gz, and, for sequences with labels, a 22 px label row. RF and ADC events never overlap in time, so the ADC
  shares the RF row instead of taking a row of its own. All rows share the display-time x-axis; each row has its label, unit,
  zero line and min/max labels on the left.
- **Units**: RF magnitude in Hz, RF phase in rad, gradients in kHz/m, time in
  ms.
- **Waveform data** comes from wasm as a per-pixel-column envelope:
  `waveforms(iters, t0, t1, columns)` resolves the blocks visible in the
  window for the current iterations and returns, per channel and column, the
  minimum and maximum value (NaN where nothing is drawn). Drawing cost
  depends on canvas width, not on sequence length.
  - Gradients use the breakpoints of their interpreted shapes (four points for
    a trapezoid); each segment between breakpoints contributes its exact
    value range to every column it crosses.
  - RF magnitude is `|amp| · |shape|`. RF phase is `arg(shape)` plus the
    event phase (plus π for a negative amplitude), wrapped to (−π, π], and
    drawn only where the magnitude is nonzero.
  - ADC is drawn in the RF row as a translucent band from the zero line to
    about half the row height over each acquisition window, with one tick per
    sample once samples are at least 4 px apart. The RF and phase rows each
    have a small RF/ADC legend in their label area.
  - ADC phase is the event phase plus the per-sample phase shape if there is
    one, wrapped to (−π, π], over the acquisition window. It is returned as
    a seventh envelope channel and drawn in the phase row in the ADC colour,
    so receiver and RF phase share one axis.
  - Blocks narrower than two columns contribute a per-block, per-channel
    (min, max) summary computed at load time instead of their samples.
    Summaries use per-shape statistics cached by shape, since pulseq-rs shares
    shapes between blocks.
- The renderer draws each column as a vertical line from min to max,
  connected to the neighbouring columns, which shows a smooth curve when
  zoomed in and the filled extent of dense waveforms when zoomed out.
- **Colours and themes**: one fixed hue per channel (RF, RF phase, Gx, Gy,
  Gz, ADC and ADC phase), all defined as CSS variables per theme and read by
  the canvas at draw time. A header menu offers System (default), Light,
  Dark, Solarized Light, Solarized Dark, Nord, Dracula and Paper; each theme takes
  its channel colours from its own palette. `data-theme` on `<html>` always
  holds a concrete theme: System resolves to Light or Dark and follows OS
  changes. The choice is remembered per browser, and an inline script in
  `index.html` applies it before first paint. Loop extents get a light
  background tint in all rows.
- **Paper style**: the Paper theme sets `--plot-style: paper`, which
  switches the canvas to publication-style axes in the manner of matplotlib
  and MATLAB figures in MRI papers:
  - white background, black text, Arial/Helvetica (`--plot-font`), and
    matplotlib's tab10 channel colours (ADC grey); no loop tints;
  - every row is a framed axes with a rotated y label including the unit
    (`Gx (kHz/m)`), outward ticks at round values (at least three per row;
    the phase row at −π, 0, π) with true minus signs, and the RF/ADC legend
    as a box in the top-right corner of the axes;
  - x ticks on the bottom edge of every row, tick labels under the last one
    and a `Time (ms)` axis title;
  - no label row (an interactive aid; the hover panel still shows labels);
  - axis breaks (a gap with two slashes in each frame) wherever real time
    jumps, with a collapsed delay's real duration beside its break instead
    of a shaded band;
  - loop brackets without loop names: `iteration 5/64`, or for stacked
    loops `64 iterations` (`32 of 64 iterations` when capped).
- **Export**: **Save PNG** redraws the plot at 3× its on-screen size without
  the hover line and downloads it as `<sequence name>.png`, in whatever
  theme is active.
- **Time axis**: ticks and labels are real sequence time in ms, placed
  through `time_segments`, a piecewise display → real mapping from wasm for
  the current iterations, with stacked loops at their first iteration.
  - Real time advances at the display rate within a piece. A collapsed delay
    is treated as cut at its centre: its first half continues the time
    before it, its second half leads into the time after it, so ticks run
    on through both halves.
  - Consecutive pieces whose real times continue each other are merged, so
    every remaining boundary is a jump in real time: the centre of a
    collapsed delay, the end of a stacked loop (it jumps over the remaining
    iterations), or a loop shown at a later iteration. Each jump gets a
    break mark: two slashes between the tick marks on screen, a gap with two
    slashes in every frame in paper style.
  - Ticks sit at round real times; a piece followed by a jump leaves its end
    tick to the next piece, and labels that would overlap the previous one
    are dropped.
- **Navigation**: wheel zooms around the cursor, horizontal wheel and drag
  pan, double-click resets to the full timeline. On touch screens a
  two-finger pinch zooms around the fingers' midpoint (keeping the time under
  it fixed, so moving both fingers also pans), one finger pans, a tap focuses
  a bracket and a double tap resets. The footer shows the block
  number and real time under the pointer, and the sequence name, block count,
  real duration, loop count and interpreter warnings.
- Redraws are scheduled with `requestAnimationFrame` and coalesced.

## wasm API

```rust
#[wasm_bindgen]
pub struct Viewer { /* interpreted sequence, loop layout, block summaries */ }

#[wasm_bindgen]
impl Viewer {
    /// Parse, interpret and analyse a .seq file. Errors become JS exceptions
    /// with the pulseq-rs message.
    pub fn load(source: &str) -> Result<Viewer, JsError>;

    pub fn name(&self) -> Option<String>;
    pub fn warnings(&self) -> Vec<String>;
    pub fn block_count(&self) -> u32;
    pub fn duration(&self) -> f64;          // real [s]
    pub fn display_duration(&self) -> f64; // display [s]

    /// Max |value| per channel, for fixed y-scales.
    pub fn channel_max(&self) -> Vec<f64>;

    /// Loops in pre-order, 8 values each: parent (-1 at top level), depth,
    /// count, blocks per iteration, first block, display start, display
    /// duration, real duration of one iteration.
    pub fn loops(&self) -> Vec<f64>;

    /// Per channel and column, (min, max) in the display window [t0, t1].
    pub fn waveforms(&self, iters: &[u32], t0: f64, t1: f64, columns: u32) -> Vec<f32>;

    /// Display times of ADC samples in the window; empty if more than `max`.
    pub fn adc_samples(&self, iters: &[u32], t0: f64, t1: f64, max: u32) -> Vec<f64>;

    /// Display → real time pieces in the window, 3 values each: display
    /// start, display end, real start; boundaries are real-time jumps.
    pub fn time_segments(&self, iters: &[u32], t0: f64, t1: f64) -> Vec<f64>;

    /// Collapse delay blocks (re-runs the layout).
    pub fn set_collapse_delays(&mut self, collapse: bool);

    /// Collapsed delays in the window, 3 values each: display start,
    /// display end, real duration.
    pub fn collapsed(&self, iters: &[u32], t0: f64, t1: f64, max: u32) -> Vec<f64>;

    /// Labels used, their values after a block, the blocks with label
    /// operations in the window (display start, event), and an event's text.
    pub fn label_names(&self) -> Vec<String>;
    pub fn labels_at(&self, block: u32) -> Vec<i32>;
    pub fn label_marks(&self, iters: &[u32], t0: f64, t1: f64, max: u32) -> Vec<f64>;
    pub fn label_text(&self, event: u32) -> String;

    /// Blocks under t over all iterations of the stacked loops enclosing it
    /// (stacked[loop] != 0), and the unique values of every label over them
    /// (per label: count, then sorted values).
    pub fn hover_blocks(&self, iters: &[u32], stacked: &[u8], t: f64) -> Vec<u32>;
    pub fn hover_labels(&self, iters: &[u32], stacked: &[u8], t: f64) -> Vec<i32>;

    /// [block index, real time, block display start] under display time t.
    pub fn hover(&self, iters: &[u32], t: f64) -> Vec<f64>;
}
```

`iters` is the frontend's `Uint32Array` of current iteration indices, one per
loop; all per-frame work (resolving blocks, building envelopes) happens in one
call.

## Examples

`web/public/examples/` holds the built-in sequences: `flash_je.seq`,
`grappa_acs.seq` and `seq_make_radial.seq` from pulseq-rs, and
`flash_repeated.seq`, generated by `scripts/make_repeated_flash.py` (the FLASH
TR loop repeated 200 times with a delay in between: ~96k blocks, two nested
loops). Vite copies the folder into the site unchanged, so the files are also
available at `<pages url>/examples/<name>.seq`. The Rust tests read the same
files.

## Build and deploy

- `npm run wasm` (in `web/`) builds the crate for `wasm32-unknown-unknown` in
  release mode and runs `wasm-bindgen --target web` into `web/src/wasm`; Vite
  bundles the module and its JS bindings. The `wasm-bindgen` CLI version
  matches the crate's pinned `wasm-bindgen` dependency.
- `npm run build` type-checks and runs `vite build` with `base: "./"`,
  producing a static site in `web/dist` that works under the repository's
  Pages subpath.
- `.github/workflows/pages.yml` runs on pull requests, pushes to `main` and
  manually: it installs Rust with the wasm target and the `wasm-bindgen` CLI,
  runs `cargo test`, builds wasm and the site, and on `main` deploys
  `web/dist` with `actions/deploy-pages`.
- Local development: `npm run wasm` once (and after Rust changes), then
  `npm run dev`.
