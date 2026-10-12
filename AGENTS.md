# slicc-jerk

`jerk`: get, list, create and drop OAuth tokens by dispatching to each provider's own CLI. Rust workspace: `crates/jerk` (bin), `jerk-core`, `jerk-oauth`, `jerk-oauth-wasm`. Native on macOS, Linux (musl) and Windows; `wasm32-wasip1` for seven's kernel; `wasm32-unknown-unknown` via wasm-bindgen for bios/vault. npm `@ai-ecoverse/slicc-jerk`, `…-<os>-<arch>` and `@ai-ecoverse/jerk-oauth`.

No comments in files we write. Agent guidance only here (≤1000 chars); no CLAUDE.md. Unit tests in gitignored `test/unit/<crate>.rs`, included under `cfg(all(test, jerk_unit))`; `npm run test:unit` needs `cargo-llvm-cov`, lefthook wants 100% diff coverage. `test/integration/` is committed and runs in CI on all three OSes.

Never read, print or use a real token, keychain or `~/.npmrc` in tests; use fake CLIs on `PATH`. Never touch a browser on 127.0.0.1:9222–9229; use a stub CDP server. Releases: semantic-release on `main`.
