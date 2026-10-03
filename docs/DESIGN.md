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
crates/seq-core/        Rust: loader glue, loop detection, waveforms, wasm API
web/                    Vite app
  src/main.ts           entry point, file input, wiring
  src/timeline.ts       display-time model built from the loop tree
  src/render.ts         canvas renderer
  src/controls.ts       loop iteration controls
  src/wasm/             wasm-pack output (generated, not committed)
docs/DESIGN.md          this document
.github/workflows/      Pages build + deploy
```

## Loading

1. The user picks a file (file input or drag and drop onto the page).
2. The page reads it as text and passes it to wasm.
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

The best repeat is the one covering the most blocks (`p·k`). Ties go to the
smaller period, then to the earlier start. `P_max` is 512 tokens; after inner
loops are collapsed (below) outer loop bodies are only a few tokens long.
Cost is `O(n · P_max)` per pass.

### Choosing where an iteration starts

A run with period `p` can be read with its body starting at any of `p`
offsets. The body is rotated so it starts at the first block in the run that
contains an RF event; the blocks before that offset stay outside the loop.
Without an RF event the run starts where the scan found it. The rotated
repeat is kept if it still has `k ≥ 2` copies.

### Collapsing and nesting

The chosen repeat is replaced in `s` by a single new token representing
`loop(body, k)`. Loop tokens are interned like signatures: two loops are the
same token exactly when they have the same body tokens and the same `k`.
The search then runs again on the shortened string, until no repeat with
`k ≥ 2` remains.

Because inner loops collapse first into single tokens, outer loops appear as
short repeats of those tokens on later passes, and nesting falls out of the
iteration: phase-encoding lines collapse into one token, then slices, then
averages.

### Result: the loop tree

The final string is the top level of a tree:

```
Node = Block(index)
     | Loop { body: [Node], count: k, first_block: index, blocks_per_iter: m }
```

Because every iteration of a loop consists of identical tokens, every
iteration spans the same number of blocks `m` and the same duration. The
concrete block for a position in the body is therefore pure arithmetic:
`first_block + iteration · m + offset`. This is what lets the frontend map an
iteration selection to block indices without asking wasm.

## Timeline model

The display axis is **display time**, built from the loop tree:

- A top-level block occupies its real duration.
- A loop occupies the duration of **one** iteration of its body, with nested
  loops inside the body also counted as one iteration.

The timeline is a list of segments, each with a display-time start, a width,
and either a block range (linear part) or a loop node. Mapping display time to
real time requires the current iteration of every enclosing loop, which the
stack-mode state provides.

## Stack mode

Stack mode shows exactly one iteration of every loop at a time.

- **State**: one iteration index per loop node, starting at 0. A loop nested
  in another loop's body has one shared index, independent of which outer
  iteration is shown.
- **Resolving blocks**: the renderer walks the timeline; for each loop
  segment it applies the current indices top-down to get the concrete block
  indices to draw.
- **Controls**:
  - Above the plot, every loop has a bracket spanning its segment, labelled
    with its current iteration and count (`7 / 128`).
  - Scrolling the mouse wheel over a bracket steps that loop's iteration;
    clicking the bracket focuses it, after which arrow keys step it.
  - A panel lists all loops, nested by depth, each with a slider and number
    field for its iteration.
- **Fixed y-scales**: each channel's y-range is the maximum over the whole
  sequence, so stepping through iterations shows amplitude changes instead of
  rescaling.

## Rendering

- **One `<canvas>`** filling the plot area, sized with `devicePixelRatio` for
  sharp lines. Rows from top to bottom: RF magnitude, RF phase, Gx, Gy, Gz,
  ADC. All rows share the display-time x-axis.
- **Units**: RF magnitude in Hz, RF phase in rad, gradients in kHz/m, time in
  ms.
- **Waveform data** comes from wasm: `waveforms(block_indices, offsets)` takes
  the resolved block indices and their display-time offsets and returns one
  polyline per channel as `Float32Array`s of interleaved `(t, value)` pairs,
  already in display time.
  - Gradients use the breakpoints of their interpreted shapes (four points for
    a trapezoid).
  - RF magnitude and phase use the RF shape samples, scaled by amplitude and
    with phase offset applied.
  - ADC is drawn as a filled bar over each acquisition window; when zoomed in
    far enough, one tick per sample.
- **Level of detail**: when a polyline has more than two points per pixel
  column, it is reduced to the min/max per column before drawing, so drawing
  cost depends on canvas width, not on sequence length.
- **Navigation**: wheel zooms around the cursor, drag pans, double-click
  resets to the full timeline. Loop segments get a light background tint so
  their extent is visible inside each row.
- Redraws are scheduled with `requestAnimationFrame` and coalesced.

## wasm API

```rust
#[wasm_bindgen]
pub struct Viewer { /* interpreted sequence + loop tree */ }

#[wasm_bindgen]
impl Viewer {
    /// Parse and analyse a .seq file. Errors become JS exceptions with a message.
    pub fn load(source: &str) -> Result<Viewer, JsError>;

    /// Number of blocks and per-block durations (real time, seconds).
    pub fn block_count(&self) -> u32;
    pub fn block_durations(&self) -> Float64Array;

    /// Loop tree, flattened in pre-order. Per node: kind, parent, count,
    /// first_block, blocks_per_iter, body range.
    pub fn loop_tree(&self) -> Uint32Array;

    /// Per-channel max |amplitude| over the whole sequence, for fixed y-scales.
    pub fn channel_ranges(&self) -> Float64Array;

    /// Polylines for the given blocks placed at the given display-time offsets.
    pub fn waveforms(&self, blocks: &[u32], offsets: &[f64]) -> Waveforms;
}
```

## Build and deploy

- `wasm-pack build crates/seq-core --target web --out-dir ../../web/src/wasm`
  produces the wasm module and its JS bindings; Vite bundles them.
- `vite build` with `base: "./"` produces a static site in `web/dist` that
  works under the repository's Pages subpath.
- `.github/workflows/pages.yml` runs on pushes to `main` (and manually): it
  installs Rust with the `wasm32-unknown-unknown` target and `wasm-pack`,
  builds wasm, builds the site, and deploys `web/dist` with
  `actions/deploy-pages`.
- Local development: `npm run dev` in `web/`, after building wasm once.
