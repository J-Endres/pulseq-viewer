pub mod labels;
pub mod loops;
pub mod model;

use wasm_bindgen::prelude::*;

use model::{Analysis, CHANNELS, NONE};

/// Fields per loop in `Viewer::loops`.
pub const LOOP_FIELDS: usize = 8;

#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct Viewer {
    a: Analysis,
}

#[wasm_bindgen]
impl Viewer {
    /// Parse, interpret and analyse a .seq file.
    pub fn load(source: &str) -> Result<Viewer, JsError> {
        Analysis::load(source)
            .map(|a| Viewer { a })
            .map_err(|e| JsError::new(&e))
    }

    pub fn name(&self) -> Option<String> {
        self.a.name.clone()
    }

    pub fn warnings(&self) -> Vec<String> {
        self.a.warnings.clone()
    }

    pub fn block_count(&self) -> u32 {
        self.a.blocks.len() as u32
    }

    /// Total real duration `[s]`.
    pub fn duration(&self) -> f64 {
        self.a.block_start.last().copied().unwrap_or(0.0)
    }

    /// Total display duration `[s]`.
    pub fn display_duration(&self) -> f64 {
        self.a.display_duration
    }

    pub fn channel_count() -> u32 {
        CHANNELS as u32
    }

    /// Max |value| per channel, in plot units.
    pub fn channel_max(&self) -> Vec<f64> {
        self.a.channel_max.to_vec()
    }

    /// Loops in pre-order, `LOOP_FIELDS` values each: parent (-1 at top
    /// level), depth, count, blocks per iteration, first block, display
    /// start `[s]`, display duration `[s]`, real duration of one iteration `[s]`.
    pub fn loops(&self) -> Vec<f64> {
        let mut out = Vec::with_capacity(self.a.loops.len() * LOOP_FIELDS);
        for l in &self.a.loops {
            let first = l.first_block as usize;
            let iter_end = first + l.blocks_per_iter as usize;
            out.extend([
                if l.parent == NONE {
                    -1.0
                } else {
                    l.parent as f64
                },
                l.depth as f64,
                l.count as f64,
                l.blocks_per_iter as f64,
                l.first_block as f64,
                l.disp_start,
                l.disp_dur,
                self.a.block_start[iter_end] - self.a.block_start[first],
            ]);
        }
        out
    }

    /// Per channel and pixel column, (min, max) in `[t0, t1]` (display time),
    /// NaN where empty. `iters` holds the current iteration of every loop.
    pub fn waveforms(&self, iters: &[u32], t0: f64, t1: f64, columns: u32) -> Vec<f32> {
        self.a.waveforms(iters, t0, t1, columns as usize)
    }

    /// Display times of ADC samples in `[t0, t1]`; empty if more than `max`.
    pub fn adc_samples(&self, iters: &[u32], t0: f64, t1: f64, max: u32) -> Vec<f64> {
        self.a.adc_samples(iters, t0, t1, max as usize)
    }

    /// Draw delay blocks (no events) at most as wide as the median block
    /// with events. Changes the display layout (`loops`, `display_duration`).
    pub fn set_collapse_delays(&mut self, collapse: bool) {
        self.a.set_collapse_delays(collapse);
    }

    /// Collapsed delay blocks in `[t0, t1]`, 3 values each: display start,
    /// display end `[s]`, real duration `[s]`; at most `max` of them.
    pub fn collapsed(&self, iters: &[u32], t0: f64, t1: f64, max: u32) -> Vec<f64> {
        self.a
            .collapsed(iters, t0, t1, max as usize)
            .into_iter()
            .flatten()
            .collect()
    }

    /// Labels the sequence sets or increments, in display order.
    pub fn label_names(&self) -> Vec<String> {
        self.a.labels.names.clone()
    }

    /// Values of `label_names` after the operations of block `block`.
    pub fn labels_at(&self, block: u32) -> Vec<i32> {
        self.a.labels.at(block as usize)
    }

    /// Blocks with label operations in `[t0, t1]`, 2 values each: display
    /// start `[s]` and an event index for `label_text`; at most `max`.
    pub fn label_marks(&self, iters: &[u32], t0: f64, t1: f64, max: u32) -> Vec<f64> {
        self.a
            .label_marks(iters, t0, t1, max as usize)
            .into_iter()
            .flat_map(|(t, e)| [t, e as f64])
            .collect()
    }

    /// The operations of a label event, e.g. `LIN=48 AVG+1`.
    pub fn label_text(&self, event: u32) -> String {
        self.a.labels.text(event as usize).to_string()
    }

    /// `[block index, real time [s], block display start [s]]` under display
    /// time `t`, or empty outside the timeline.
    pub fn hover(&self, iters: &[u32], t: f64) -> Vec<f64> {
        match self.a.hover(iters, t) {
            Some((b, real, start)) => vec![b as f64, real, start],
            None => Vec::new(),
        }
    }
}
