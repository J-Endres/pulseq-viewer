import type { State } from "./state.ts";

type Legend = readonly (readonly [string, string])[];

/** Rows; row i shows channel i of `Viewer.waveforms`. */
const ROWS: readonly { label: string; unit: string; color: string; weight: number; symmetric: boolean; legend?: Legend }[] = [
  { label: "RF", unit: "Hz", color: "--ch-rf", weight: 1, symmetric: false, legend: [["RF", "--ch-rf"], ["ADC", "--ch-adc"]] },
  { label: "Phase", unit: "rad", color: "--ch-phase", weight: 0.7, symmetric: true, legend: [["RF", "--ch-phase"], ["ADC", "--ch-adc"]] },
  { label: "Gx", unit: "kHz/m", color: "--ch-gx", weight: 1, symmetric: true },
  { label: "Gy", unit: "kHz/m", color: "--ch-gy", weight: 1, symmetric: true },
  { label: "Gz", unit: "kHz/m", color: "--ch-gz", weight: 1, symmetric: true },
];
const RF_ROW = 0;
const PHASE_ROW = 1;
/** Channels without a row of their own: ADC windows (drawn in the RF row,
 * which they never overlap) and receiver phase (drawn in the phase row). */
const ADC = 5;
const ADC_PHASE = 6;

/** Layout in CSS pixels. */
const GUTTER = 104;
const RIGHT = 12;
const BRACKET_ROW = 22;
const BRACKET_TOP = 6;
const AXIS = 34;
const ROW_GAP = 8;
/** Height of the label row, shown only for sequences with labels. */
const LABEL_ROW = 22;

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Bracket {
  loop: number;
  x0: number;
  x1: number;
  y: number;
}

/** Draws the timeline for a `State` onto a canvas and exposes its geometry. */
export class Plot {
  private ctx: CanvasRenderingContext2D;
  private width = 0;
  private height = 0;
  private dpr = 1;
  private frame = 0;
  private channelMax: Float64Array;
  private hasLabels: boolean;
  /** Bracket hit boxes from the last draw. */
  brackets: Bracket[] = [];

  constructor(
    readonly canvas: HTMLCanvasElement,
    readonly state: State,
  ) {
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Canvas 2D is not available");
    this.ctx = ctx;
    this.channelMax = state.viewer.channel_max();
    this.hasLabels = state.viewer.label_names().length > 0;
  }

  resize(width: number, height: number): void {
    this.dpr = window.devicePixelRatio || 1;
    this.width = width;
    this.height = height;
    this.canvas.width = Math.round(width * this.dpr);
    this.canvas.height = Math.round(height * this.dpr);
    this.canvas.style.width = `${width}px`;
    this.canvas.style.height = `${height}px`;
    this.request();
  }

  /** Schedule a redraw on the next animation frame (coalesced). */
  request(): void {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.draw();
    });
  }

  get bracketHeight(): number {
    return BRACKET_TOP + (this.state.maxDepth + 1) * BRACKET_ROW;
  }

  /** Plot area (all channel rows) in CSS pixels. */
  get area(): Rect {
    const y = this.bracketHeight;
    return {
      x: GUTTER,
      y,
      w: Math.max(1, this.width - GUTTER - RIGHT),
      h: Math.max(1, this.height - y - AXIS),
    };
  }

  /** CSS x → display time. */
  timeAt(x: number): number {
    const a = this.area;
    return this.state.t0 + ((x - a.x) / a.w) * (this.state.t1 - this.state.t0);
  }

  /** Display time → CSS x. */
  xAt(t: number): number {
    const a = this.area;
    return a.x + ((t - this.state.t0) / (this.state.t1 - this.state.t0)) * a.w;
  }

  /** Loop whose bracket is at CSS (x, y), or -1. */
  bracketAt(x: number, y: number): number {
    for (const b of this.brackets) {
      if (x >= b.x0 - 3 && x <= b.x1 + 3 && Math.abs(y - b.y) <= BRACKET_ROW / 2) return b.loop;
    }
    return -1;
  }

  /** Label row at the bottom of the plot area, or null without labels. */
  private labelRow(): Rect | null {
    if (!this.hasLabels) return null;
    const a = this.area;
    return { x: a.x, y: a.y + a.h - LABEL_ROW, w: a.w, h: LABEL_ROW };
  }

  private rows(): Rect[] {
    const a = this.area;
    const total = ROWS.reduce((s, r) => s + r.weight, 0);
    const labels = this.hasLabels ? LABEL_ROW + ROW_GAP : 0;
    const free = a.h - labels - ROW_GAP * (ROWS.length - 1);
    let y = a.y;
    return ROWS.map((r) => {
      const h = (free * r.weight) / total;
      const rect = { x: a.x, y, w: a.w, h };
      y += h + ROW_GAP;
      return rect;
    });
  }

  draw(): void {
    const { ctx, state } = this;
    const css = getComputedStyle(this.canvas);
    const color = (name: string) => css.getPropertyValue(name).trim();
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    ctx.fillStyle = color("--surface");
    ctx.fillRect(0, 0, this.width, this.height);
    ctx.font = `12px ${css.fontFamily}`;
    ctx.textBaseline = "middle";

    const a = this.area;
    const rows = this.rows();

    // Loop tints behind the rows
    ctx.save();
    ctx.beginPath();
    ctx.rect(a.x, a.y, a.w, a.h);
    ctx.clip();
    for (const loop of state.loops) {
      const x0 = this.xAt(loop.dispStart);
      const x1 = this.xAt(loop.dispStart + loop.dispDur);
      if (x1 < a.x || x0 > a.x + a.w) continue;
      ctx.fillStyle = color(loop.id === state.focused ? "--loop-tint-focus" : "--loop-tint");
      ctx.fillRect(x0, a.y, x1 - x0, a.h);
    }
    ctx.restore();

    this.drawCollapsed(color);
    this.drawBrackets(color);

    // Waveforms
    const columns = Math.max(1, Math.round(a.w * this.dpr));
    const data = state.viewer.waveforms(state.iters, state.t0, state.t1, columns);
    rows.forEach((rect, ch) => {
      const row = ROWS[ch]!;
      this.drawRowFrame(rect, ch, color);
      const slice = data.subarray(ch * columns * 2, (ch + 1) * columns * 2);
      ctx.save();
      ctx.beginPath();
      ctx.rect(rect.x, rect.y - 1, rect.w, rect.h + 2);
      ctx.clip();
      if (ch === RF_ROW) {
        const adc = data.subarray(ADC * columns * 2, (ADC + 1) * columns * 2);
        this.drawAdc(adc, columns, rect, this.yScale(ch, rect), color("--ch-adc"));
      }
      this.drawEnvelope(slice, columns, rect, this.yScale(ch, rect), color(row.color));
      if (ch === PHASE_ROW) {
        const adcPhase = data.subarray(ADC_PHASE * columns * 2, (ADC_PHASE + 1) * columns * 2);
        this.drawEnvelope(adcPhase, columns, rect, this.yScale(ch, rect), color("--ch-adc"));
      }
      ctx.restore();
    });

    this.drawLabels(color);
    this.drawAxis(color);

    // Hover line
    if (state.hover !== null) {
      const x = this.xAt(state.hover);
      if (x >= a.x && x <= a.x + a.w) {
        ctx.strokeStyle = color("--text-muted");
        ctx.lineWidth = 1;
        ctx.setLineDash([3, 3]);
        ctx.beginPath();
        ctx.moveTo(Math.round(x) + 0.5, a.y);
        ctx.lineTo(Math.round(x) + 0.5, a.y + a.h);
        ctx.stroke();
        ctx.setLineDash([]);
      }
    }
  }

  /** Collapsed delay blocks: a tinted band with dashed edges and the real duration. */
  private drawCollapsed(color: (n: string) => string): void {
    const { ctx, state } = this;
    const a = this.area;
    const d = state.viewer.collapsed(state.iters, state.t0, state.t1, 5000);
    if (d.length === 0) return;
    ctx.save();
    ctx.beginPath();
    ctx.rect(a.x, a.y, a.w, a.h);
    ctx.clip();
    ctx.font = `11px ${getComputedStyle(this.canvas).fontFamily}`;
    ctx.textAlign = "center";
    ctx.textBaseline = "bottom";
    ctx.setLineDash([2, 3]);
    ctx.lineWidth = 1;
    for (let i = 0; i < d.length; i += 3) {
      const x0 = this.xAt(d[i]!);
      const x1 = this.xAt(d[i + 1]!);
      ctx.fillStyle = color("--collapsed");
      ctx.fillRect(x0, a.y, x1 - x0, a.h);
      if (x1 - x0 < 4) continue;
      ctx.strokeStyle = color("--bracket");
      ctx.beginPath();
      for (const x of [x0, x1]) {
        ctx.moveTo(Math.round(x) + 0.5, a.y);
        ctx.lineTo(Math.round(x) + 0.5, a.y + a.h);
      }
      ctx.stroke();
      const label = formatSeconds(d[i + 2]!);
      if (ctx.measureText(label).width + 6 <= x1 - x0) {
        ctx.fillStyle = color("--text-muted");
        // Above the label row, which has text of its own
        ctx.fillText(label, (x0 + x1) / 2, (this.labelRow()?.y ?? a.y + a.h) - 2);
      }
    }
    ctx.restore();
  }

  /** Label row: a tick at every block that sets or increments labels, with
   * its operations as text where there is room before the next tick. */
  private drawLabels(color: (n: string) => string): void {
    const rect = this.labelRow();
    if (!rect) return;
    const { ctx, state } = this;
    const font = getComputedStyle(this.canvas).fontFamily;
    ctx.fillStyle = color("--text");
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    ctx.font = `600 12px ${font}`;
    ctx.fillText("Labels", 8, rect.y + rect.h / 2);
    ctx.strokeStyle = color("--grid");
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(rect.x, Math.round(rect.y) + 0.5);
    ctx.lineTo(rect.x + rect.w, Math.round(rect.y) + 0.5);
    ctx.stroke();

    const marks = state.viewer.label_marks(state.iters, state.t0, state.t1, 2000);
    ctx.save();
    ctx.beginPath();
    ctx.rect(rect.x, rect.y, rect.w, rect.h);
    ctx.clip();
    ctx.font = `11px ${font}`;
    const xs: number[] = [];
    for (let i = 0; i < marks.length; i += 2) xs.push(Math.round(this.xAt(marks[i]!)) + 0.5);
    ctx.strokeStyle = color("--accent");
    ctx.beginPath();
    for (const x of xs) {
      ctx.moveTo(x, rect.y + 3);
      ctx.lineTo(x, rect.y + rect.h - 3);
    }
    ctx.stroke();
    ctx.fillStyle = color("--text-muted");
    xs.forEach((x, k) => {
      const end = Math.min(xs[k + 1] ?? Infinity, rect.x + rect.w) - 6;
      const room = end - (x + 4);
      if (room < 24) return;
      let text = state.viewer.label_text(marks[2 * k + 1]!);
      if (ctx.measureText(text).width > room) {
        while (text.length > 1 && ctx.measureText(`${text}…`).width > room) text = text.slice(0, -1);
        text += "…";
      }
      ctx.fillText(text, x + 4, rect.y + rect.h / 2);
    });
    ctx.restore();
  }

  /** Value → CSS y within a row. */
  private yScale(ch: number, rect: Rect): (v: number) => number {
    const row = ROWS[ch]!;
    const max = this.channelMax[ch]! || 1;
    const lo = row.symmetric ? -max : 0;
    const pad = 0.06 * rect.h;
    return (v) => rect.y + pad + (1 - (v - lo) / (max - lo)) * (rect.h - 2 * pad);
  }

  private drawRowFrame(rect: Rect, ch: number, color: (n: string) => string): void {
    const { ctx } = this;
    const row = ROWS[ch]!;
    ctx.fillStyle = color("--text");
    ctx.textAlign = "left";
    ctx.font = `600 12px ${getComputedStyle(this.canvas).fontFamily}`;
    ctx.fillText(row.label, 8, rect.y + Math.min(10, rect.h / 2));
    ctx.font = `11px ${getComputedStyle(this.canvas).fontFamily}`;
    if (row.unit) {
      ctx.fillStyle = color("--text-muted");
      ctx.fillText(row.unit, 8, rect.y + Math.min(24, rect.h / 2 + 12));
    }
    if (row.legend && rect.h >= 56) {
      // Legend: colour swatch next to text in text colour
      row.legend.forEach(([name, swatch], i) => {
        const y = rect.y + 38 + i * 13;
        ctx.strokeStyle = color(swatch);
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(8, y);
        ctx.lineTo(18, y);
        ctx.stroke();
        ctx.fillStyle = color("--text-muted");
        ctx.fillText(name, 22, y);
      });
    }

    // Zero line and y labels
    const y = this.yScale(ch, rect);
    const max = this.channelMax[ch]!;
    ctx.strokeStyle = color("--grid");
    ctx.lineWidth = 1;
    ctx.beginPath();
    const y0 = Math.round(y(0)) + 0.5;
    ctx.moveTo(rect.x, y0);
    ctx.lineTo(rect.x + rect.w, y0);
    ctx.stroke();
    ctx.fillStyle = color("--text-muted");
    ctx.textAlign = "right";
    const ticks = ROWS[ch]!.symmetric ? [max, 0, -max] : [max, 0];
    for (const v of ticks) {
      if (rect.h < 40 && v !== 0) continue;
      ctx.fillText(formatValue(v), rect.x - 6, y(v));
    }
  }

  private drawEnvelope(
    d: Float32Array,
    columns: number,
    rect: Rect,
    y: (v: number) => number,
    stroke: string,
  ): void {
    const { ctx } = this;
    const step = rect.w / columns;
    ctx.strokeStyle = stroke;
    ctx.lineWidth = 1.5;
    ctx.lineJoin = "round";
    ctx.beginPath();
    let drawing = false;
    let lastY = 0;
    for (let c = 0; c < columns; c++) {
      const lo = d[2 * c]!;
      if (Number.isNaN(lo)) {
        drawing = false;
        continue;
      }
      const x = rect.x + (c + 0.5) * step;
      const yLo = y(lo);
      const yHi = y(d[2 * c + 1]!);
      // Enter the column at the end nearer the previous point.
      const [first, second] = Math.abs(yLo - lastY) < Math.abs(yHi - lastY) ? [yLo, yHi] : [yHi, yLo];
      if (drawing) ctx.lineTo(x, first);
      else ctx.moveTo(x, first);
      ctx.lineTo(x, second);
      lastY = second;
      drawing = true;
    }
    ctx.stroke();
  }

  /** ADC windows as a band rising from the zero line, plus sample ticks. */
  private drawAdc(d: Float32Array, columns: number, rect: Rect, y: (v: number) => number, fill: string): void {
    const { ctx, state } = this;
    const step = rect.w / columns;
    const bottom = y(0);
    const top = rect.y + (bottom - rect.y) * 0.45;
    const h = bottom - top;
    ctx.fillStyle = fill;
    ctx.globalAlpha = 0.25;
    let start = -1;
    for (let c = 0; c <= columns; c++) {
      const on = c < columns && !Number.isNaN(d[2 * c]!);
      if (on && start < 0) start = c;
      if (!on && start >= 0) {
        ctx.fillRect(rect.x + start * step, top, Math.max(1, (c - start) * step), h);
        start = -1;
      }
    }
    ctx.globalAlpha = 1;
    // Individual samples once they are far enough apart
    const samples = state.viewer.adc_samples(state.iters, state.t0, state.t1, Math.floor(rect.w / 4));
    ctx.strokeStyle = fill;
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (const t of samples) {
      const x = Math.round(this.xAt(t)) + 0.5;
      ctx.moveTo(x, top);
      ctx.lineTo(x, top + h);
    }
    ctx.stroke();
  }

  private drawBrackets(color: (n: string) => string): void {
    const { ctx, state } = this;
    const a = this.area;
    this.brackets = [];
    ctx.save();
    ctx.beginPath();
    ctx.rect(a.x, 0, a.w, this.bracketHeight);
    ctx.clip();
    for (const loop of state.loops) {
      const x0 = Math.max(a.x - 10, this.xAt(loop.dispStart));
      const x1 = Math.min(a.x + a.w + 10, this.xAt(loop.dispStart + loop.dispDur));
      if (x1 < a.x || x0 > a.x + a.w) continue;
      const y = BRACKET_TOP + loop.depth * BRACKET_ROW + BRACKET_ROW / 2;
      this.brackets.push({ loop: loop.id, x0, x1, y });
      const focused = loop.id === state.focused;
      ctx.strokeStyle = color(focused ? "--accent" : "--bracket");
      ctx.lineWidth = focused ? 2 : 1.25;
      ctx.beginPath();
      ctx.moveTo(x0 + 0.5, y + 6);
      ctx.lineTo(x0 + 0.5, y);
      ctx.lineTo(x1 - 0.5, y);
      ctx.lineTo(x1 - 0.5, y + 6);
      ctx.stroke();

      const iter = (state.iters[loop.id] ?? 0) + 1;
      const label = `Loop ${loop.name} · ${iter} / ${loop.count}`;
      const short = `${iter}/${loop.count}`;
      const visible0 = Math.max(x0, a.x);
      const visible1 = Math.min(x1, a.x + a.w);
      const room = visible1 - visible0 - 8;
      const text = ctx.measureText(label).width <= room ? label : ctx.measureText(short).width <= room ? short : "";
      if (!text) continue;
      const tw = ctx.measureText(text).width;
      const cx = (visible0 + visible1) / 2;
      ctx.fillStyle = color("--surface");
      ctx.fillRect(cx - tw / 2 - 4, y - 8, tw + 8, 16);
      ctx.fillStyle = color(focused ? "--accent" : "--text");
      ctx.textAlign = "center";
      ctx.font = `${focused ? 600 : 400} 12px ${getComputedStyle(this.canvas).fontFamily}`;
      ctx.fillText(text, cx, y);
    }
    ctx.restore();
  }

  private drawAxis(color: (n: string) => string): void {
    const { ctx, state } = this;
    const a = this.area;
    const y = a.y + a.h + 6;
    const spanMs = (state.t1 - state.t0) * 1e3;
    const step = niceStep(spanMs / Math.max(2, a.w / 90));
    ctx.fillStyle = color("--text-muted");
    ctx.strokeStyle = color("--grid");
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    ctx.font = `11px ${getComputedStyle(this.canvas).fontFamily}`;
    const first = Math.ceil((state.t0 * 1e3) / step) * step;
    ctx.beginPath();
    for (let ms = first; ms <= state.t1 * 1e3; ms += step) {
      const x = Math.round(this.xAt(ms / 1e3)) + 0.5;
      ctx.moveTo(x, y - 6);
      ctx.lineTo(x, y - 2);
      const label = formatMs(ms, step);
      const half = ctx.measureText(label).width / 2;
      if (x - half >= GUTTER - 2 && x + half <= this.width - 2) ctx.fillText(label, x, y);
    }
    ctx.stroke();
    ctx.textAlign = "right";
    ctx.fillText("ms", GUTTER - 6, y);
    ctx.textBaseline = "middle";
  }
}

function formatSeconds(s: number): string {
  if (s >= 1) return `${s.toPrecision(3)} s`;
  if (s >= 1e-3) return `${(s * 1e3).toPrecision(3)} ms`;
  return `${(s * 1e6).toPrecision(3)} µs`;
}

function niceStep(raw: number): number {
  const exp = Math.pow(10, Math.floor(Math.log10(raw)));
  const f = raw / exp;
  return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * exp;
}

function formatMs(ms: number, step: number): string {
  const decimals = Math.max(0, -Math.floor(Math.log10(step)));
  return (Math.abs(ms) < step / 2 ? 0 : ms).toFixed(decimals);
}

function formatValue(v: number): string {
  if (v === 0) return "0";
  const abs = Math.abs(v);
  if (abs >= 1e5 || abs < 0.01) return v.toExponential(1);
  if (abs >= 100) return Math.round(v).toString();
  const s = v.toPrecision(3);
  return s.includes(".") ? s.replace(/\.?0+$/, "") : s;
}
