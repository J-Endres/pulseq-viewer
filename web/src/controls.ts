import { cmapGradient } from "./colormap.ts";
import type { State } from "./state.ts";

/**
 * Loop panel: per loop, nested by depth, a slider and number field for its
 * iteration, or with Stack pressed a colour bar for its overlaid iterations.
 * Returns a function that redraws theme-dependent colours.
 */
export function buildLoopPanel(container: HTMLElement, state: State): () => void {
  container.replaceChildren();
  if (state.loops.length === 0) {
    const empty = document.createElement("p");
    empty.className = "muted";
    empty.textContent = "No repeating blocks found.";
    container.append(empty);
    return () => {};
  }

  const items = state.loops.map((loop) => {
    const item = document.createElement("div");
    item.className = "loop";
    item.style.setProperty("--depth", String(loop.depth));

    const head = document.createElement("button");
    head.type = "button";
    head.className = "loop-head";
    head.innerHTML = `<span class="loop-name"></span><span class="loop-meta"></span>`;
    head.querySelector(".loop-name")!.textContent = `Loop ${loop.name}`;
    head.querySelector(".loop-meta")!.textContent =
      `×${loop.count} · ${loop.blocksPerIter} block${loop.blocksPerIter === 1 ? "" : "s"} · ${formatDuration(loop.iterDur)}`;
    head.addEventListener("click", () => state.setFocus(state.focused === loop.id ? -1 : loop.id));

    const stack = document.createElement("button");
    stack.type = "button";
    stack.className = "stack-toggle";
    stack.textContent = "Stack";
    stack.title = "Overlay all iterations of this loop";
    stack.addEventListener("click", () => state.setStacked(loop.id, !state.stacked[loop.id]));
    const top = document.createElement("div");
    top.className = "loop-top";
    top.append(head, stack);

    const row = document.createElement("div");
    row.className = "loop-input";
    const slider = document.createElement("input");
    slider.type = "range";
    slider.min = "1";
    slider.max = String(loop.count);
    slider.value = "1";
    slider.setAttribute("aria-label", `Loop ${loop.name} iteration`);
    const number = document.createElement("input");
    number.type = "number";
    number.min = "1";
    number.max = String(loop.count);
    number.value = "1";
    number.setAttribute("aria-label", `Loop ${loop.name} iteration number`);
    const set = (v: string) => {
      const n = Number(v);
      if (Number.isFinite(n)) state.setIter(loop.id, n - 1);
    };
    slider.addEventListener("input", () => set(slider.value));
    number.addEventListener("change", () => set(number.value));
    slider.addEventListener("focus", () => state.setFocus(loop.id));
    number.addEventListener("focus", () => state.setFocus(loop.id));
    row.append(slider, number);

    // Shown instead of the slider while stacked: colour bar 1 … count
    const scale = document.createElement("div");
    scale.className = "loop-scale";
    scale.innerHTML = `<span>1</span><span class="bar"></span><span></span>`;
    scale.lastElementChild!.textContent = String(loop.count);
    const bar = scale.querySelector<HTMLElement>(".bar")!;

    item.append(top, row, scale);
    container.append(item);
    return { item, slider, number, stack, row, scale, bar };
  });

  const sync = () => {
    state.loops.forEach((loop, i) => {
      const { item, slider, number, stack, row, scale } = items[i]!;
      const v = String((state.iters[loop.id] ?? 0) + 1);
      if (slider.value !== v) slider.value = v;
      if (number.value !== v && document.activeElement !== number) number.value = v;
      item.classList.toggle("focused", state.focused === loop.id);
      const stacked = state.stacked[loop.id] === 1;
      stack.setAttribute("aria-pressed", String(stacked));
      row.hidden = stacked;
      scale.hidden = !stacked;
    });
  };
  const recolor = () => {
    const gradient = cmapGradient(getComputedStyle(container));
    for (const { bar } of items) bar.style.background = gradient;
  };
  state.onChange((change) => {
    if (change === "iters" || change === "focus") sync();
    if (change === "focus" && state.focused >= 0) {
      items[state.focused]?.item.scrollIntoView({ block: "nearest" });
    }
  });
  sync();
  recolor();
  return recolor;
}

export function formatDuration(s: number): string {
  if (s >= 1) return `${s.toFixed(s >= 10 ? 1 : 2)} s`;
  if (s >= 1e-3) return `${(s * 1e3).toFixed(s >= 1e-2 ? 1 : 2)} ms`;
  return `${(s * 1e6).toFixed(0)} µs`;
}
