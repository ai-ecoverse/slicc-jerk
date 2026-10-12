# slicc-jerk

`jerk`: get, list, create and drop OAuth tokens by dispatching to each provider's own CLI. Rust workspace: `crates/jerk` (bin), `crates/jerk-core`. Native on macOS, Linux (musl) and Windows; `wasm32-wasip1` for seven's kernel (`slicc.abi` `wasi`). npm `@ai-ecoverse/slicc-jerk` plus `@ai-ecoverse/slicc-jerk-<os>-<arch>`.

No comments in files we write. Agent guidance only here (≤1000 chars); no CLAUDE.md. Unit tests live in the gitignored `test/unit/<crate>.rs`, included under `cfg(all(test, jerk_unit))`; `npm run test:unit` needs `cargo-llvm-cov`, and lefthook wants 100% diff coverage. `test/integration/` is committed and runs in CI on all three OSes.

Never read, print or use a real token, keychain or `~/.npmrc` in tests; use fake CLIs on `PATH`. Never touch a browser on 127.0.0.1:9222–9229; use a stub CDP server. Releases: semantic-release on `main`.
