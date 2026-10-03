import "./style.css";
import init, { Viewer } from "./wasm/seq_core.js";
import { State } from "./state.ts";
import { Plot } from "./render.ts";
import { buildLoopPanel, formatDuration } from "./controls.ts";
import { EXAMPLES, fetchExample } from "./examples.ts";
import { initTheme } from "./theme.ts";

const $ = <T extends HTMLElement>(sel: string) => {
  const el = document.querySelector<T>(sel);
  if (!el) throw new Error(`${sel} missing`);
  return el;
};

const fileInput = $<HTMLInputElement>("#file");
const fileName = $("#file-name");
const exampleSelect = $<HTMLSelectElement>("#example");
const collapseInput = $<HTMLInputElement>("#collapse");
const plotHost = $("#plot");
const canvas = $<HTMLCanvasElement>("#canvas");
const message = $("#message");
const panel = $("#loops");
const readout = $("#readout");
const info = $("#info");
const loopsSection = $("#loops-section");
const labelsSection = $("#labels-section");
const labelsTitle = $("#labels-title");
const labelsList = $("#labels");

const wasmReady = init();

interface Shown {
  state: State;
  plot: Plot;
  /** Redraw theme-dependent colours outside the canvas. */
  recolor: () => void;
  dispose: () => void;
}

let current: Shown | null = null;

// The canvas reads its colours from CSS variables at draw time.
initTheme($<HTMLSelectElement>("#theme"), () => {
  current?.plot.request();
  current?.recolor();
});

function showMessage(text: string, kind: "hint" | "error" | "busy"): void {
  message.textContent = text;
  message.dataset.kind = kind;
  message.hidden = false;
}

/** Increments on every load so a slower earlier load cannot replace a newer one. */
let loadId = 0;

async function open(name: string, readText: () => Promise<string>): Promise<void> {
  const id = ++loadId;
  fileName.textContent = name;
  showMessage(`Reading ${name}…`, "busy");
  // Let the message paint before the synchronous analysis.
  await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)));
  try {
    const [text] = await Promise.all([readText(), wasmReady]);
    if (id !== loadId) return;
    const viewer = Viewer.load(text);
    current?.dispose();
    current = show(viewer);
    if (import.meta.env.DEV) Object.assign(window, { pulseq: current });
    message.hidden = true;
  } catch (e) {
    if (id !== loadId) return;
    const msg = e instanceof Error ? e.message : String(e);
    showMessage(`Could not load ${name}: ${msg}`, "error");
    console.error(e);
  }
}

function openFile(file: File): void {
  exampleSelect.value = "";
  void open(file.name, () => file.text());
}

function show(viewer: Viewer): Shown {
  viewer.set_collapse_delays(collapseInput.checked);
  const state = new State(viewer);
  const plot = new Plot(canvas, state);
  canvas.hidden = false;
  const recolor = buildLoopPanel(panel, state);

  const warnings = viewer.warnings();
  info.replaceChildren();
  const parts = [
    viewer.name(),
    `${viewer.block_count().toLocaleString()} blocks`,
    formatDuration(viewer.duration()),
    `${state.loops.length} loop${state.loops.length === 1 ? "" : "s"}`,
  ].filter(Boolean);
  info.append(parts.join(" · "));
  if (warnings.length) {
    const w = document.createElement("span");
    w.className = "warn";
    w.textContent = ` · ${warnings.length} warning${warnings.length === 1 ? "" : "s"}`;
    w.title = warnings.join("\n");
    info.append(w);
  }

  state.onChange((change) => {
    plot.request();
    if (change === "hover" || change === "iters") updateReadout(state);
  });

  const resize = new ResizeObserver(() => {
    plot.resize(plotHost.clientWidth, plotHost.clientHeight);
  });
  resize.observe(plotHost);

  const ac = new AbortController();
  attachInteraction(plot, state, ac.signal);
  updateReadout(state);

  return {
    state,
    plot,
    recolor,
    dispose: () => {
      ac.abort();
      resize.disconnect();
      viewer.free();
    },
  };
}

function updateReadout(state: State): void {
  const t = state.hover;
  const h = t === null ? null : state.viewer.hover(state.iters, t);
  if (t === null || !h || h.length === 0) {
    readout.textContent = "";
    showLabels(state, null);
    return;
  }
  // Under a stacked loop, the pointer covers one block per iteration.
  const blocks = state.anyStacked ? state.viewer.hover_blocks(state.iters, state.stacked, t) : null;
  if (blocks && blocks.length > 1) {
    readout.textContent = `${blocks.length} blocks overlaid (blocks ${blocks[0]! + 1} … ${blocks[blocks.length - 1]! + 1})`;
    showStackedLabels(state, t, blocks.length);
    return;
  }
  readout.textContent = `block ${h[0]! + 1} · t = ${(h[1]! * 1e3).toFixed(3)} ms`;
  showLabels(state, h[0]!);
}

/** While hovering a block, the side panel shows label values instead of loops. */
function showLabels(state: State, block: number | null): void {
  const names = state.viewer.label_names();
  const show = block !== null && names.length > 0;
  loopsSection.hidden = show;
  labelsSection.hidden = !show;
  if (!show) return;
  const values = state.viewer.labels_at(block);
  const before = block > 0 ? state.viewer.labels_at(block - 1) : new Int32Array(names.length);
  labelsTitle.textContent = `Labels · block ${block + 1}`;
  // Highlight values this block changed
  fillLabels(names.map((name, i) => [name, String(values[i]), values[i] !== before[i]]));
}

/** Label values over all overlaid iterations at display time `t`. */
function showStackedLabels(state: State, t: number, blocks: number): void {
  const names = state.viewer.label_names();
  const show = names.length > 0;
  loopsSection.hidden = show;
  labelsSection.hidden = !show;
  if (!show) return;
  const flat = state.viewer.hover_labels(state.iters, state.stacked, t);
  const rows: [string, string, boolean][] = [];
  let k = 0;
  for (const name of names) {
    const n = flat[k++]!;
    const values = Array.from(flat.subarray(k, k + n));
    k += n;
    // Labels that differ between iterations are highlighted
    rows.push([name, formatValues(values), n > 1]);
  }
  labelsTitle.textContent = `Labels · ${blocks} blocks`;
  fillLabels(rows);
}

function fillLabels(rows: [string, string, boolean][]): void {
  labelsList.replaceChildren(
    ...rows.flatMap(([name, value, highlight]) => {
      const dt = document.createElement("dt");
      const dd = document.createElement("dd");
      dt.textContent = name;
      dd.textContent = value;
      dt.classList.toggle("changed", highlight);
      dd.classList.toggle("changed", highlight);
      return [dt, dd];
    }),
  );
}

/** Sorted unique values as a list, or as ranges when there are many. */
export function formatValues(v: number[]): string {
  if (v.length <= 6) return v.join(", ");
  const runs: [number, number][] = [];
  for (const x of v) {
    const last = runs[runs.length - 1];
    if (last && x === last[1] + 1) last[1] = x;
    else runs.push([x, x]);
  }
  if (runs.length <= 4) return runs.map(([a, b]) => (a === b ? `${a}` : `${a}–${b}`)).join(", ");
  return `${v[0]}–${v[v.length - 1]} (${v.length} values)`;
}

function attachInteraction(plot: Plot, state: State, signal: AbortSignal): void {
  const opts = { signal };
  const pos = (e: MouseEvent) => {
    const r = canvas.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };
  const inBrackets = (y: number) => y < plot.bracketHeight;

  let wheelAcc = 0;
  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      const { x, y } = pos(e);
      const scale = e.deltaMode === 1 ? 30 : e.deltaMode === 2 ? 300 : 1;
      const dy = e.deltaY * scale;
      const dx = e.deltaX * scale;
      const loop = inBrackets(y) ? plot.bracketAt(x, y) : -1;
      if (loop >= 0) {
        wheelAcc += dy;
        const steps = Math.trunc(wheelAcc / 40);
        if (steps !== 0) {
          wheelAcc -= steps * 40;
          if (!state.stacked[loop]) state.stepIter(loop, steps);
        }
        return;
      }
      const span = state.t1 - state.t0;
      if (Math.abs(dx) > Math.abs(dy)) {
        const shift = (dx / plot.area.w) * span;
        state.setView(state.t0 + shift, state.t1 + shift);
        return;
      }
      const t = plot.timeAt(x);
      const f = Math.exp(dy * 0.0015);
      state.setView(t - (t - state.t0) * f, t + (state.t1 - t) * f);
    },
    { signal, passive: false },
  );

  // Active pointers (CSS position), for one-finger/mouse pan and two-finger pinch.
  const pointers = new Map<number, { x: number; y: number }>();
  let drag: { x: number; t0: number; t1: number; moved: boolean } | null = null;
  let pinch: { dist: number; t: number; span: number } | null = null;
  // Set once a gesture became a pinch, until all pointers are up.
  let pinched = false;
  let lastTap = 0;

  const twoPointers = () => {
    const [p, q] = [...pointers.values()];
    return { cx: (p!.x + q!.x) / 2, dist: Math.max(10, Math.hypot(p!.x - q!.x, p!.y - q!.y)) };
  };

  canvas.addEventListener(
    "pointerdown",
    (e) => {
      if (e.pointerType === "mouse" && e.button !== 0) return;
      canvas.setPointerCapture(e.pointerId);
      pointers.set(e.pointerId, pos(e));
      if (pointers.size === 2) {
        const { cx, dist } = twoPointers();
        pinch = { dist, t: plot.timeAt(cx), span: state.t1 - state.t0 };
        pinched = true;
        drag = null;
      } else if (pointers.size === 1) {
        drag = { x: pos(e).x, t0: state.t0, t1: state.t1, moved: false };
      }
    },
    opts,
  );
  canvas.addEventListener(
    "pointermove",
    (e) => {
      const { x, y } = pos(e);
      const a = plot.area;
      if (pointers.has(e.pointerId)) pointers.set(e.pointerId, { x, y });
      if (pinch && pointers.size >= 2) {
        // Keep the time under the fingers' midpoint under the midpoint.
        const { cx, dist } = twoPointers();
        const span = (pinch.span * pinch.dist) / dist;
        const t0 = pinch.t - ((cx - a.x) / a.w) * span;
        state.setView(t0, t0 + span);
        return;
      }
      state.setHover(x >= a.x && x <= a.x + a.w ? plot.timeAt(x) : null);
      canvas.style.cursor = inBrackets(y) && plot.bracketAt(x, y) >= 0 ? "pointer" : drag?.moved ? "grabbing" : "crosshair";
      if (!drag) return;
      const dxPx = x - drag.x;
      if (Math.abs(dxPx) > 3) drag.moved = true;
      if (drag.moved) {
        const shift = (-dxPx / a.w) * (drag.t1 - drag.t0);
        state.setView(drag.t0 + shift, drag.t1 + shift);
      }
    },
    opts,
  );
  const release = (e: PointerEvent) => {
    pointers.delete(e.pointerId);
    if (pointers.size < 2) pinch = null;
    const wasGesture = pinched || drag?.moved;
    drag = null;
    if (pointers.size === 0) pinched = false;
    return !wasGesture;
  };
  canvas.addEventListener(
    "pointerup",
    (e) => {
      if (!release(e)) return;
      // A tap: focus the bracket under it; on touch, a double tap resets the view.
      const { x, y } = pos(e);
      const loop = inBrackets(y) ? plot.bracketAt(x, y) : -1;
      state.setFocus(loop === state.focused ? -1 : loop);
      if (e.pointerType === "touch") {
        if (e.timeStamp - lastTap < 300) state.resetView();
        lastTap = e.timeStamp;
      }
    },
    opts,
  );
  canvas.addEventListener("pointercancel", (e) => void release(e), opts);
  canvas.addEventListener("pointerleave", () => state.setHover(null), opts);
  canvas.addEventListener("dblclick", () => state.resetView(), opts);

  window.addEventListener(
    "keydown",
    (e) => {
      if (e.target instanceof HTMLInputElement) return;
      const id = state.focused;
      if (id < 0) return;
      const loop = state.loops[id]!;
      const step: Record<string, number> = { ArrowRight: 1, ArrowUp: 1, ArrowLeft: -1, ArrowDown: -1 };
      if (state.stacked[id] && (e.key in step || e.key === "Home" || e.key === "End")) return;
      if (e.key in step) state.stepIter(id, step[e.key]!);
      else if (e.key === "Home") state.setIter(id, 0);
      else if (e.key === "End") state.setIter(id, loop.count - 1);
      else if (e.key === "Escape") state.setFocus(-1);
      else return;
      e.preventDefault();
    },
    opts,
  );
}

// Remembered per browser; storage may be unavailable (private mode).
try {
  const saved = localStorage.getItem("collapseDelays");
  if (saved !== null) collapseInput.checked = saved === "1";
} catch {}
collapseInput.addEventListener("change", () => {
  try {
    localStorage.setItem("collapseDelays", collapseInput.checked ? "1" : "0");
  } catch {}
  current?.state.setCollapseDelays(collapseInput.checked);
});

fileInput.addEventListener("change", () => {
  const file = fileInput.files?.[0];
  if (file) openFile(file);
  fileInput.value = "";
});

// Drag and drop anywhere on the page
let dragDepth = 0;
window.addEventListener("dragenter", (e) => {
  e.preventDefault();
  dragDepth++;
  document.body.classList.add("dropping");
});
window.addEventListener("dragleave", () => {
  if (--dragDepth <= 0) {
    dragDepth = 0;
    document.body.classList.remove("dropping");
  }
});
window.addEventListener("dragover", (e) => e.preventDefault());
window.addEventListener("drop", (e) => {
  e.preventDefault();
  dragDepth = 0;
  document.body.classList.remove("dropping");
  const file = e.dataTransfer?.files[0];
  if (file) openFile(file);
});

for (const [i, example] of EXAMPLES.entries()) {
  exampleSelect.add(new Option(example.label, String(i)));
}
exampleSelect.addEventListener("change", () => {
  const example = EXAMPLES[Number(exampleSelect.value)];
  if (example) void open(example.file, () => fetchExample(example));
});

showMessage("Open a .seq file, drop it here, or pick an example. Files are read locally and never uploaded.", "hint");
