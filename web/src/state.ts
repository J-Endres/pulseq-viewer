import type { Viewer } from "./wasm/seq_core.js";

/** Fields per loop in `Viewer.loops()`. */
const LOOP_FIELDS = 8;

export interface Loop {
  id: number;
  /** Hierarchical name, e.g. "2.1". */
  name: string;
  /** Parent loop id, -1 at top level. */
  parent: number;
  depth: number;
  count: number;
  blocksPerIter: number;
  firstBlock: number;
  /** Display time [s]. */
  dispStart: number;
  dispDur: number;
  /** Real duration of one iteration [s]. */
  iterDur: number;
}

export type Change = "iters" | "view" | "focus" | "hover" | "layout";

/** Everything the plot and the controls show, plus change notification. */
export class State {
  loops: Loop[];
  readonly maxDepth: number;
  /** Current iteration of every loop (stack mode). */
  readonly iters: Uint32Array;
  /** Visible display-time window [s]. */
  t0 = 0;
  t1 = 1;
  /** Focused loop id, or -1. */
  focused = -1;
  /** Display time under the pointer, or null. */
  hover: number | null = null;

  private listeners: ((change: Change) => void)[] = [];

  constructor(readonly viewer: Viewer) {
    this.loops = parseLoops(viewer.loops());
    this.maxDepth = this.loops.reduce((m, l) => Math.max(m, l.depth), -1);
    this.iters = new Uint32Array(this.loops.length);
    this.resetView();
  }

  /** Collapse or expand delay blocks; loops keep their iterations. */
  setCollapseDelays(collapse: boolean): void {
    this.viewer.set_collapse_delays(collapse);
    this.loops = parseLoops(this.viewer.loops());
    this.emit("layout");
    this.resetView();
  }

  get displayDuration(): number {
    return this.viewer.display_duration();
  }

  onChange(fn: (change: Change) => void): void {
    this.listeners.push(fn);
  }

  private emit(change: Change): void {
    for (const fn of this.listeners) fn(change);
  }

  setIter(id: number, iter: number): void {
    const loop = this.loops[id];
    if (!loop) return;
    const clamped = Math.max(0, Math.min(loop.count - 1, Math.round(iter)));
    if (this.iters[id] === clamped) return;
    this.iters[id] = clamped;
    this.emit("iters");
  }

  stepIter(id: number, delta: number): void {
    this.setIter(id, (this.iters[id] ?? 0) + delta);
  }

  setFocus(id: number): void {
    if (this.focused === id) return;
    this.focused = id;
    this.emit("focus");
  }

  setView(t0: number, t1: number): void {
    const total = this.displayDuration;
    const minSpan = 1e-6;
    const span = Math.max(minSpan, Math.min(t1 - t0, total * 1.1 || minSpan));
    const margin = total * 0.05;
    t0 = Math.max(-margin, Math.min(t0, total + margin - span));
    this.t0 = t0;
    this.t1 = t0 + span;
    this.emit("view");
  }

  resetView(): void {
    const total = this.displayDuration || 1;
    this.setView(-total * 0.01, total * 1.01);
  }

  setHover(t: number | null): void {
    if (this.hover === t) return;
    this.hover = t;
    this.emit("hover");
  }
}

function parseLoops(flat: Float64Array): Loop[] {
  const loops: Loop[] = [];
  const childCount = new Map<number, number>();
  for (let i = 0; i * LOOP_FIELDS < flat.length; i++) {
    const f = flat.subarray(i * LOOP_FIELDS, (i + 1) * LOOP_FIELDS);
    const parent = f[0]!;
    const n = (childCount.get(parent) ?? 0) + 1;
    childCount.set(parent, n);
    const prefix = parent < 0 ? "" : `${loops[parent]!.name}.`;
    loops.push({
      id: i,
      name: `${prefix}${n}`,
      parent,
      depth: f[1]!,
      count: f[2]!,
      blocksPerIter: f[3]!,
      firstBlock: f[4]!,
      dispStart: f[5]!,
      dispDur: f[6]!,
      iterDur: f[7]!,
    });
  }
  return loops;
}
