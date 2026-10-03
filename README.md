# Pulseq Viewer

Browser-based viewer for [Pulseq](https://pulseq.github.io/) `.seq` files that
shows unrolled loops rolled up. Runs entirely client-side.

See [docs/DESIGN.md](docs/DESIGN.md) for how it works.

## Development

```sh
cd web
npm install
npm run dev
```

`npm run build` writes the static site to `web/dist`. Pushes to `main` deploy
it to GitHub Pages via `.github/workflows/pages.yml`.
