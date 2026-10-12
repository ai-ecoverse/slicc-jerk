# slicc-jerk

Get, list, create and drop OAuth tokens across operating systems and inside SLICC, by dispatching to each provider's own CLI.

`jerk` (as in an ice-cream jerk) asks `gh`, `npm`, `wrangler`, `aio` and friends for their tokens, so that adding a provider is configuration, not code. It feeds [fnox](https://fnox.jdx.dev) as a `command` lease backend on a real machine. Inside seven, tokens go to the vault's sealed store, and programs only ever see placeholders.

**Status:** being built. `jerk list`, `jerk config`, and `get`, `list`, `create` and `drop` for providers backed by a CLI (gh, npm, wrangler, aio) work. OAuth providers (loopback, auto, device modes) work for `get`, `create` and `drop`. `lease`, `sync` and `intercept` print `not implemented yet` and exit 5 until they land.

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

## OAuth providers

A provider can use an `[oauth]` block instead of CLI commands. jerk handles the OAuth flow itself: PKCE loopback, device code, or auto (loopback when the port binds, otherwise exit 4).

```toml
[providers.xai]
name = "xAI Grok"
env = ["XAI_API_KEY"]
domains = ["api.x.ai"]
[providers.xai.oauth]
mode = "auto"
flow = "pkce"
authorize_url = "https://auth.x.ai/oauth2/auth?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}"
token_url = "https://auth.x.ai/oauth2/token"
client_id = "b1a00492-073a-47ea-816f-4c329264a828"
redirect_uri = "http://127.0.0.1:56121/callback"
scope = "openid profile email offline_access grok-cli:access api:access"
[providers.xai.oauth.capture]
at = "request"
from = "query"
param = "code"
```

`jerk xai create` opens the sign-in in the system browser. The refresh token goes to the OS keyring (service `slicc-jerk`, account `xai/default`), or to a file store (mode 0600) with `store = "file"`. `jerk xai get` refreshes the token at `token_url` and prints the access token by the same rules as CLI providers. `jerk xai drop --yes` removes the grant.

The `device` mode sends the first request to `authorize_url` as the device authorization endpoint, prints the verification URI and user code on stderr, then polls `token_url`. `intercept` and `popup` modes exit 5 until they land.

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
