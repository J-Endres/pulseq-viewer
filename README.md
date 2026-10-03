# Pulseq Viewer

Browser-based viewer for [Pulseq](https://pulseq.github.io/) `.seq` files that
shows unrolled loops rolled up. Runs entirely client-side.

See [docs/DESIGN.md](docs/DESIGN.md) for how it works.

## Development

Requires Rust with the `wasm32-unknown-unknown` target, the matching
`wasm-bindgen` CLI, and Node.js:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked

cd web
npm install
npm run wasm   # build the Rust crate to web/src/wasm (again after Rust changes)
npm run dev
```

`cargo test` runs the loop detection tests. `npm run build` writes the static
site to `web/dist`. Pushes to `main` deploy it to GitHub Pages via
`.github/workflows/pages.yml`.
