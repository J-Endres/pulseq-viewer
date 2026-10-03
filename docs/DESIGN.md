# Pulseq Viewer — Design

A static web page, hosted on GitHub Pages, that opens a Pulseq `.seq` file and
plots it. Its distinguishing feature is that loops unrolled in the file (phase
encoding, slices, averages, …) are detected and shown rolled up: the timeline
runs left to right, and each detected loop occupies the width of a single
iteration, with controls to step through its iterations.

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
            │   loop controls (stack mode)             │
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
  src/model.rs          loading, signatures, display layout, waveforms
  src/lib.rs            wasm API
  tests/                tests against the example .seq files
web/                    Vite app
  src/main.ts           entry point, file loading, pointer/keyboard input
  src/state.ts          loops, iteration indices, view window, change events
  src/render.ts         canvas renderer
  src/controls.ts       loop panel
  src/examples.ts       built-in example list
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

A run with period `p` can be read with its body starting at any of `p`
offsets. The body is rotated so it starts at the first block in the run that
contains an RF event; the blocks before that offset stay outside the loop.
If the run has no RF block, or the rotation would leave fewer than two
copies, the run starts where the scan found it.

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

## Stack mode

Stack mode shows exactly one iteration of every loop at a time.

- **State**: one iteration index per loop node, starting at 0. A loop nested
  in another loop's body has one shared index, independent of which outer
  iteration is shown.
- **Controls**:
  - Above the plot, every loop has a bracket spanning its segment, one row per
    nesting depth, labelled with its name and current iteration
    (`Loop 1.2 · 7 / 128`). Loops are named by position: top-level loops
    `1`, `2`, …; loops inside loop `1` are `1.1`, `1.2`, ….
  - Scrolling the mouse wheel over a bracket steps that loop's iteration;
    clicking the bracket focuses it, after which arrow keys step it (Home and
    End jump to the first and last iteration, Escape clears the focus).
  - A side panel lists all loops, nested by depth, each with its count,
    blocks per iteration, real duration of one iteration, and a slider and
    number field for its iteration. The focused loop is highlighted in both
    the panel and the plot.
- **Fixed y-scales**: each channel's y-range is the maximum over the whole
  sequence, so stepping through iterations shows amplitude changes instead of
  rescaling.

## Rendering

- **One `<canvas>`** filling the plot area, sized with `devicePixelRatio` for
  sharp lines. Rows from top to bottom: RF magnitude, RF phase, Gx, Gy, Gz,
  ADC. All rows share the display-time x-axis; each row has its label, unit,
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
  - ADC is drawn as a bar over each acquisition window, with one tick per
    sample once samples are at least 4 px apart.
  - Blocks narrower than two columns contribute a per-block, per-channel
    (min, max) summary computed at load time instead of their samples.
    Summaries use per-shape statistics cached by shape, since pulseq-rs shares
    shapes between blocks.
- The renderer draws each column as a vertical line from min to max,
  connected to the neighbouring columns, which shows a smooth curve when
  zoomed in and the filled extent of dense waveforms when zoomed out.
- **Colours**: one fixed hue per channel (RF blue, phase violet, Gx orange,
  Gy aqua, Gz magenta, ADC green), with separate light and dark values chosen
  by `prefers-color-scheme`. Loop extents get a light background tint in all
  rows.
- **Navigation**: wheel zooms around the cursor, horizontal wheel and drag
  pan, double-click resets to the full timeline. The footer shows the block
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
