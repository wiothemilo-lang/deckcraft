# Browser startup regression

`Web startup` builds the actual `deckcraft-web` bundle and opens it in Chromium. It covers the
sample and blank-document routes, both with automatic renderer selection and forced WebGL.
This catches runtime failures such as #959 (`std::process::id()` on WASM) that `cargo check`
cannot detect.

Each case requires the loading overlay to be removed, a visible resized canvas, the renderer
initialization log, and no browser exceptions or error logs. Forced WebGL cases also check the
reported backend. Console logs and screenshots are kept for three days as CI artifacts. The
built site includes the repository's license and attribution files.

The runner uses a software ANGLE renderer and a single worker. It does not certify hardware
WebGPU, every browser, or slideshow/media fidelity. The ordinary Rust CI remains required.

To run against an existing `dist/web` build, with Node 20+ and Python 3 available:

```sh
cd packaging/web/tests
npm ci --ignore-scripts --no-audit --no-fund
npx --no-install playwright install chromium
npm test
```

To create that site first, run `trunk build --release --locked` in `apps/deckcraft-web` with the
WASM Rust target installed. The workflow uses checksum-verified Trunk 0.22.0, the Cargo lockfile,
locked browser-test packages and the same pinned open font revision as the other builds.
The test runner starts its own loopback-only static server and does not reuse an existing server.
