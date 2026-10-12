# slicc-jerk

Get, list, create and drop OAuth tokens across operating systems and inside SLICC, by dispatching to each provider's own CLI.

`jerk` (as in an ice-cream jerk) asks `gh`, `npm`, `wrangler`, `aio` and friends for their tokens, so that adding a provider is configuration, not code. It feeds [fnox](https://fnox.jdx.dev) as a `command` lease backend on a real machine. Inside seven, tokens go to the vault's sealed store, and programs only ever see placeholders.

**Status:** being built. This release has the command-line skeleton, the exit codes and the packages. The commands below print `not implemented yet` and exit 5 until they land.

## Usage

```
jerk list [--json]
jerk <provider> get    [--scope S] [--reveal] [--refresh] [--login] [--json]
jerk <provider> list   [--json]
jerk <provider> create [--scope S] [--force]
jerk <provider> drop   [<id>] [--lease] [--yes]
jerk <provider> lease
jerk <provider> sync --fnox <SECRET> --provider <fnox provider>
jerk config [path|check|fnox|show <provider>]
jerk intercept --authorize-url <url> --redirect <url>
```

| exit | meaning |
| --- | --- |
| 0 | ok |
| 1 | error |
| 2 | usage, config or policy refusal |
| 3 | needs an interactive sign-in |
| 4 | not available here (no CLI, no browser, no linked slicc CLI) |
| 5 | unsupported for this provider or runtime |

## Install

- **npm:** `npm install -g @ai-ecoverse/slicc-jerk`. The package is a small Node launcher; npm picks one of the platform packages `@ai-ecoverse/slicc-jerk-<os>-<arch>` (darwin, linux, win32; arm64, x64). The same package carries `bin/jerk.wasm` for seven's kernel.
- **slicc:** the slicc CLI installs jerk with it, or with `slicc install jerk`.
- **GitHub releases:** one binary per platform, with `SHA256SUMS` and build provenance.

## Development

- `npm run lint`: slicc lint, rustfmt and clippy.
- `npm test`: builds jerk and runs `test/integration/`.
- `npm run build:wasm`: writes `bin/jerk.wasm`.
- `npm run test:unit`: the gitignored unit tests in `test/unit/`, with coverage through `cargo-llvm-cov`.

## License

Apache-2.0
