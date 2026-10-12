# slicc-jerk

Get, list, create and drop OAuth tokens across operating systems and inside SLICC, by dispatching to each provider's own CLI.

`jerk` (as in an ice-cream jerk) asks `gh`, `npm`, `wrangler`, `aio` and friends for their tokens, so that adding a provider is configuration, not code. It feeds [fnox](https://fnox.jdx.dev) as a `command` lease backend on a real machine. Inside seven, tokens go to the vault's sealed store, and programs only ever see placeholders.

**Status:** being built. `jerk list`, `jerk config`, and `get`, `list`, `create` and `drop` for providers backed by a CLI (gh, npm, wrangler, aio) work, and so do `lease`, `drop --lease`, `sync` and `config fnox`. OAuth providers and `intercept` print `not implemented yet` and exit 5 until they land.

`jerk <provider> get` prints the token only to a terminal, or with `--reveal`. To a pipe it prints a masked form such as `gho_…1234`. A token already in the provider's first `env` variable (`GH_TOKEN`, …) wins over the CLI, as gh does. `--scope` checks the granted scopes, with GitHub's implied scopes understood, and exits 3 when they don't cover it. jerk never signs in by itself: `get` exits 3, and `get --login` on a terminal runs `create` first. The provider's CLI runs without the provider's own token variables in its environment.

`jerk npm get` reads `//registry.npmjs.org/:_authToken` from `$NPM_CONFIG_USERCONFIG` or `~/.npmrc`, expanding `${VAR}` references, because `npm config get` won't print auth keys. `jerk npm create` mints a token with `npm token create`. It prints the token by the same rules as `get` (it exists nowhere else), and records only its id in `<state>/minted.json` (mode 0600), so `jerk npm drop <id>` can revoke it.

## Usage

```
jerk list [--json]
jerk <provider> get    [--scope S] [--reveal] [--refresh] [--login] [--json]
jerk <provider> list   [--json]
jerk <provider> create [--scope S] [--force]
jerk <provider> drop   [<id>] [--lease] [--yes]
jerk <provider> lease
jerk <provider> sync --fnox <SECRET> --provider <fnox provider>
jerk config path [--json] | check | show <provider> | fnox
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

## Providers

A provider is a TOML table. Layers are read from lowest to highest priority, and a provider defined again in a later layer replaces the earlier one as a whole. A system provider with `locked = true` can't be replaced.

| layer | macOS and Linux | Windows |
| --- | --- | --- |
| built-in | `github` (gh), `npm`, `cloudflare` (wrangler), `adobe-aio` (aio) | same |
| system | `/etc/slicc-jerk/providers.d/*.toml`, in file-name order (`SLICC_JERK_SYSTEM_DIR` overrides) | `%ProgramData%\slicc-jerk\providers.d\*.toml` |
| user | `$XDG_CONFIG_HOME/slicc-jerk/providers.toml`, default `~/.config/slicc-jerk/providers.toml` | `%APPDATA%\slicc-jerk\providers.toml` |
| explicit | `--config <file>` or `SLICC_JERK_CONFIG` | same |

```toml
[providers.github]
name = "GitHub (gh)"
env = ["GH_TOKEN", "GITHUB_TOKEN"]
domains = ["api.github.com", "github.com"]
requires = "gh"
params = { host = "github.com" }
get = { run = ["gh", "auth", "token", "--hostname", "{host}"], parse = "text" }
create = { run = ["gh", "auth", "login", "--web", "--hostname", "{host}", "--scopes", "{scope}"], interactive = true }
drop = { run = ["gh", "auth", "logout", "--hostname", "{host}", "--user", "{id}"], interactive = true }
```

Commands are argument lists, never shell strings. `{name}` placeholders come from `params` or from `scope`, `id`, `name`, `expires` and `user`. `jerk config check` validates every layer, `jerk config show <provider>` prints the provider that wins, and `jerk config path` shows where each layer lives.

## fnox

jerk feeds [fnox](https://fnox.jdx.dev) as a `command` lease backend. `jerk config fnox [--proxy] [<provider>...]` prints the stanzas to paste into `~/.config/fnox/config.toml`:

```toml
[leases.github]
type = "command"
create_command = "jerk github lease"
revoke_command = "jerk github drop --lease"
duration = "1h"
```

- **`jerk <provider> lease`** prints fnox's lease JSON: every `env` name mapped to the token, an `expires_at` that is the earlier of the token's real expiry and now + `FNOX_LEASE_DURATION`, and a `lease_id`. It refuses to run on a terminal.
- **`lease_via = "create"`** mints a fresh token per lease.
- **`jerk <provider> drop --lease`** reads `FNOX_LEASE_ID`. It revokes upstream only a token jerk minted for that lease. A token jerk only read is left to expire.
- **`--proxy`** adds `[[proxy.rules]]` for the provider's domains. They need a fnox build that lets proxy rules read lease credentials (F2, carried on ai-ecoverse/fnox).
- **Until then, `jerk <provider> sync --fnox <SECRET> --provider <fnox provider> [--global]`** pipes the token into `fnox set` on stdin. It only writes to providers that encrypt or use an OS store (age, keychain, 1password, bitwarden, fido2, yubikey and the cloud KMSes), never `plain`.

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
