/** Viridis, sampled at 9 evenly spaced points. */
const VIRIDIS = ["#440154", "#482878", "#3e4a89", "#31688e", "#26828e", "#1f9e89", "#35b779", "#6ece58", "#fde725"].map(
  (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)),
);

/**
 * Colour for position `f` in [0, 1], mapped into the theme's usable part of
 * viridis (`--cmap-from` / `--cmap-to`, so the ends stay visible on the
 * theme's background).
 */
export function cmap(f: number, css: CSSStyleDeclaration): string {
  const from = Number(css.getPropertyValue("--cmap-from")) || 0;
  const to = Number(css.getPropertyValue("--cmap-to")) || 1;
  const x = (from + Math.min(1, Math.max(0, f)) * (to - from)) * (VIRIDIS.length - 1);
  const i = Math.min(VIRIDIS.length - 2, Math.floor(x));
  const t = x - i;
  const [a, b] = [VIRIDIS[i]!, VIRIDIS[i + 1]!];
  const c = a.map((v, k) => Math.round(v + (b[k]! - v) * t));
  return `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
}

/** CSS linear-gradient over the theme's colour map range. */
export function cmapGradient(css: CSSStyleDeclaration): string {
  const stops = Array.from({ length: 9 }, (_, i) => cmap(i / 8, css));
  return `linear-gradient(to right, ${stops.join(", ")})`;
}
