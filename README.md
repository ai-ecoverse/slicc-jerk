# slicc-jerk

`jerk` gets, lists, creates and drops OAuth tokens on macOS, Linux and Windows, and inside SLICC. It asks each provider's own CLI (`gh`, `npm`, `wrangler`, `aio`, …) for its token, or runs the OAuth flow itself when there is no CLI. Adding a provider is configuration, not code.

The name is the ice-cream kind: a soda jerk serves you what you asked for.

## Install

```sh
curl -fsSL https://get.sliccy.ai/jerk | sh
```

The installer puts `jerk` in `~/.local/bin` (`INSTALL_DIR` or `--dir` to change it, `VERSION` or `--version` to pin one), checks it against the release's `SHA256SUMS`, and never uses `sudo`. Options pass through: `curl -fsSL https://get.sliccy.ai/jerk | sh -s -- --help`. If get.sliccy.ai is unreachable, use the release asset: `curl -fsSL https://github.com/ai-ecoverse/slicc-jerk/releases/latest/download/install.sh | sh`.

Other ways:

- **npm:** `npm install -g @ai-ecoverse/slicc-jerk`. A small Node launcher picks the platform package `@ai-ecoverse/slicc-jerk-<os>-<arch>`.
- **slicc:** the slicc CLI installs jerk with it, or with `slicc install jerk`.
- **GitHub releases:** plain binaries `jerk-<os>-<arch>` with `SHA256SUMS` and build provenance. Linux binaries are static (musl). macOS binaries are signed with a Developer ID (`ai.sliccy.jerk`) and notarized. Windows binaries are unsigned; SmartScreen may warn on first run.

## Quick start

```sh
jerk list                       # every provider, and whether its CLI is installed
jerk github create              # sign in through gh
jerk github get                 # the token on a terminal, masked in a pipe
curl -H "Authorization: Bearer $(jerk github get --reveal)" https://api.github.com/user
jerk github get --scope repo    # exit 3 unless the token covers repo
jerk github drop                # sign gh out (asks first)
```

## Commands

```
jerk list [--json]
jerk <provider> get    [--scope S] [--reveal] [--refresh] [--login] [--json] [-p k=v]
jerk <provider> list   [--json]
jerk <provider> create [--scope S] [--force] [--name N] [--expires D] [--cdp URL] [-p k=v]
jerk <provider> drop   [<id>] [--yes]
jerk <provider> lease
jerk <provider> drop --lease
jerk <provider> sync --fnox <SECRET> --provider <fnox provider> [--global]
jerk mcp:<server> create [--scope S] [--no-browser] | get | drop | import-pi
jerk config path [--json] | check | show <provider> | fnox [--proxy] [<provider>...]
jerk intercept --authorize-url U --redirect R [--rewrite m=k=v]... [--pkce] [--exchange --token-url T --client-id C] [--cdp URL]
jerk intercept --from-file F.toml
```

`--config <file>` (or `SLICC_JERK_CONFIG`) adds a providers file on top of the others. `-p k=v` sets a provider parameter, for example `-p host=github.example.com`.

### What `get` prints

| caller | output |
| --- | --- |
| stdout is a terminal | the token |
| a pipe, with `--reveal` | the token |
| a pipe, without `--reveal` | a masked form such as `gho_…1234`, so `get` is a safe status check |
| `--json` | `{provider, source, masked, expires_at, scopes}`, plus `token` only with `--reveal` |
| `--refresh` | a status line, never the token |

A token already in the provider's first `env` variable (`GH_TOKEN`, …) wins over the CLI, the way gh itself behaves. jerk never signs in on its own: when there's no token, `get` exits 3 and names the `create` to run. `get --login` on a terminal runs `create` first.

### Exit codes

| code | meaning |
| --- | --- |
| 0 | ok |
| 1 | error |
| 2 | usage, config or policy refusal |
| 3 | needs an interactive sign-in, or the token doesn't cover `--scope` |
| 4 | not available here: the CLI is missing or too old, no browser, no linked slicc CLI |
| 5 | unsupported for this provider or runtime |

## Built-in providers

| id | backed by | `get` | notes |
| --- | --- | --- | --- |
| `github` | `gh` | `gh auth token` | `--scope` understands GitHub's implied scopes (`repo` covers `public_repo`). Sign-out is `gh auth logout`. |
| `npm` | `npm` | `//registry.npmjs.org/:_authToken` from `$NPM_CONFIG_USERCONFIG` or `~/.npmrc` | `npm config get` won't print auth keys, so jerk reads the file. `create` mints a granular token (npm 11.6.3 or newer) and records only its id, so `drop <id>` can revoke it. |
| `cloudflare` | `wrangler` | `wrangler auth token --json` | Needs wrangler 4.57.0 or newer for `create` and `get`, and 4.65.0 for `list`. An older wrangler exits 4 before signing in, not 3 after. Tokens are treated as expiring after 50 minutes. |
| `adobe-aio` | `aio` | `aio login --bare --no-open` | The expiry comes from the IMS token itself. |
| `aem` | jerk's own OAuth | loopback body capture of `authToken` | Admin JWT for `api.aem.live`. Needs `-p org=… -p site=…`. |
| `xai` | jerk's own OAuth | PKCE through a loopback redirect | The sign-in opens in your browser. The grant is kept in the OS keyring. |

`jerk config show <provider>` prints the definition that's in effect.

## Configuring providers

Providers are TOML tables. Layers load from lowest to highest priority, and a provider defined in a later layer replaces the earlier one as a whole. A system provider with `locked = true` can't be replaced.

| layer | macOS and Linux | Windows |
| --- | --- | --- |
| built-in | compiled into jerk | same |
| system | `/etc/slicc-jerk/providers.d/*.toml` (`SLICC_JERK_SYSTEM_DIR` overrides) | `%ProgramData%\slicc-jerk\providers.d\*.toml` |
| user | `$XDG_CONFIG_HOME/slicc-jerk/providers.toml`, default `~/.config/slicc-jerk/providers.toml` | `%APPDATA%\slicc-jerk\providers.toml` |
| explicit | `--config <file>` or `SLICC_JERK_CONFIG` | same |

`jerk config check` validates every layer, and `jerk config path` shows where each one lives.

### A CLI provider

```toml
[providers.github]
name = "GitHub (gh)"
env = ["GH_TOKEN", "GITHUB_TOKEN"]
domains = ["api.github.com", "github.com"]
requires = "gh"
params = { host = "github.com" }
get = { run = ["gh", "auth", "token", "--hostname", "{host}"], parse = "text" }
scopes = { run = ["gh", "auth", "status", "--hostname", "{host}", "--active", "--json", "hosts"], parse = "json:/hosts/{host}/0/scopes" }
create = { run = ["gh", "auth", "login", "--web", "--hostname", "{host}", "--scopes", "{scope}"], interactive = true }
drop = { run = ["gh", "auth", "logout", "--hostname", "{host}", "--user", "{id}"], interactive = true }
```

Commands are argument lists, never shell strings, and the CLI runs without the provider's own token variables in its environment. `{name}` placeholders come from `params`, or from `scope`, `id`, `name`, `expires` and `user`. An argument whose placeholder is empty is dropped, together with the flag before it.

| provider field | meaning |
| --- | --- |
| `name` | display name |
| `env` | variable names for the token; the first one is checked before the CLI |
| `domains` | the hosts the token is for (used for fnox proxy rules) |
| `requires` | the CLI that must be on `PATH` |
| `params` | template defaults, overridable with `-p` |
| `max_age` | expiry to assume when the CLI doesn't report one, such as `50m` |
| `store` | `keyring` (default) or `file`, for grants jerk keeps itself |
| `lease_via` | `get` (default) or `create`, to mint a fresh token per fnox lease |
| `locked` | system files only: later layers can't replace this provider |
| `get`, `list`, `scopes`, `user`, `create`, `drop` | one command each |

| command field | meaning |
| --- | --- |
| `run` | the argument list |
| `read` | `get` only: `{ ini = path, key = k }` reads a value from an ini file instead |
| `parse` | `text`, `jwt` or `json:<pointer>` |
| `expiry` | `jwt` to read the expiry from the token |
| `timeout` | such as `10s` |
| `min_version` | the oldest CLI version that supports this command (exit 4 below it) |
| `interactive` | the command keeps the terminal (sign-ins) |
| `link` | a `create` that may run from seven over the slicc link |
| `id`, `lookup` | for a `create` that mints a token: where to find its id for revocation |

### An OAuth provider

A provider can have an `[oauth]` block instead of CLI commands. jerk then runs the flow itself.

```toml
[providers.xai]
name = "xAI Grok"
env = ["XAI_API_KEY"]
domains = ["api.x.ai"]
[providers.xai.oauth]
mode = "auto"
flow = "pkce"
authorize_url = "https://auth.x.ai/oauth2/authorize?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}"
token_url = "https://auth.x.ai/oauth2/token"
client_id = "b1a00492-073a-47ea-816f-4c329264a828"
redirect_uri = "http://127.0.0.1:56121/callback"
scope = "openid profile email offline_access grok-cli:access api:access"
capture = { at = "request", from = "query", param = "code" }
```

| `mode` | how the sign-in comes back |
| --- | --- |
| `loopback` | jerk listens on the `redirect_uri` port and opens your browser |
| `device` | RFC 8628: jerk prints a URL and a code on stderr and polls `token_url` |
| `intercept` | jerk drives a browser over CDP and catches the redirect, for clients whose loopback port is taken or that only redirect somewhere jerk can't listen |
| `auto` | `loopback` when the port is free, otherwise `intercept` |

`create` signs in and keeps the grant: in the OS keyring (service `slicc-jerk`), or with `store = "file"` in a 0600 file. `get` refreshes the token at `token_url` when it's close to expiring. `drop` removes the grant. On Linux the keyring is the kernel keyring, which doesn't survive a reboot.

`jerk aem create -p org=… -p site=…` uses the aem-cli loopback login (`client_id=aem-cli`, redirect `/.aem/cli/login/ack`). After you click Send, jerk stores the admin JWT (`authToken`) for `api.aem.live` as `AEM_TOKEN` (`X-Auth-Token` / `Authorization: token …`). The login host defaults to `admin.hlx.page` (Helix 6 sites still use that login endpoint today); override with `-p login_host=…` when `api.aem.live` returns `authToken` itself. Body capture CORS allow-list is the provider's `cors_origins`; any `{placeholder}` in `authorize_url` beyond the built-in OAuth vars must be supplied with `-p`.

For `intercept`, the browser is the one in `--cdp`, otherwise `SLICC_CDP_URL`, otherwise a running browser from `slicc browsers --json` (the one with seven open first), otherwise exit 4.

### MCP servers

Every server with a `url` in pi's `~/.pi/agent/mcp.json` (or `PI_MCP_CONFIG`) is a provider named `mcp:<server>`. A server given only by URL is `mcp:url:<origin+path>`. The token's variable is `MCP_<SERVER>_TOKEN`.

```sh
jerk mcp:linear create            # discovers the authorization server, registers a client, signs in
jerk mcp:linear get --reveal --json
jerk mcp:linear import-pi         # copy an existing sign-in from pi's mcp-auth.json
```

`create` follows the MCP authorization spec:
- it finds the authorization server through protected-resource metadata (RFC 9728) and its metadata (RFC 8414);
- it registers through mcpcat's client metadata document, or dynamic registration, or uses the `clientId` from `mcp.json`;
- it signs in with PKCE, sending the server as `resource` (RFC 8707).

`--no-browser` prints the URL and reads the redirected URL from stdin. pi's `oauth` fields in `mcp.json` are honoured. mcpcat uses these providers whenever jerk is installed.

## fnox

jerk feeds [fnox](https://fnox.jdx.dev) as a `command` lease backend. `jerk config fnox` prints the stanzas for `~/.config/fnox/config.toml`:

```toml
[leases.github]
type = "command"
create_command = "jerk github lease"
revoke_command = "jerk github drop --lease"
duration = "1h"
```

- **`lease`** prints fnox's lease JSON: every `env` name mapped to the token, an `expires_at` that is the earlier of the token's own expiry and now + `FNOX_LEASE_DURATION`, and a `lease_id`. It refuses a terminal.
- **`drop --lease`** reads `FNOX_LEASE_ID` and revokes upstream only a token jerk minted for that lease. A token jerk only read is left to expire.
- **`sync --fnox <SECRET> --provider <p>`** pipes the token into `fnox set` on stdin. That's how jerk tokens reach `fnox proxy`, which reads only `[secrets]`. It writes only to fnox providers that encrypt or use an OS store (age, keychain, 1password, bitwarden, fido2, yubikey and the cloud KMSes), never `plain`. Run it again when the token is refreshed.
- **`config fnox --proxy`** also prints `[[proxy.rules]]` for the provider's domains.
- **On Windows,** fnox's lease backend runs `sh -c`, so `fnox exec` with jerk leases needs an `sh` on `PATH` (Git for Windows, MSYS2 or similar).

## In SLICC

The npm package also carries `bin/jerk.wasm`, a `wasm32-wasip1` build that seven runs as a kernel command (`slicc.abi` `wasi`). That jerk never holds a token: tokens live in the vault's sealed store, and kernel programs only ever see placeholders.

- **`jerk list`** shows seven's own `[oauth]` providers and, when a slicc CLI is linked, the providers of the machine behind it.
- **`jerk <provider> get`** prints the placeholder.
- **`create`** asks the linked machine's jerk to sign in, or bios to run an `[oauth]` sign-in, and the result is sealed.
- **`drop`** removes the sealed record.
- **`<provider> list`, `lease`, `sync` and `intercept`** exit 5.

All of this goes through `/dev/sealed`.

On the machine side, the slicc CLI runs jerk with `--via-link`:
- `create --via-link` prints progress lines and a final status as newline-delimited JSON, and never prompts. It refuses `--force`, minted tokens, and sign-ins that need a terminal.
- `lease --via-link` prints the lease, then a status.
- `drop --lease --via-link` prints a status. A CLI sign-out is never run over the link.

## Security

- **Printing tokens:** jerk prints a token only to a terminal or with `--reveal`, and never logs one. Errors never repeat tokens, codes, verifiers, `state`, config values or response bodies.
- **Tokens at rest:** jerk stores no access tokens of CLI providers; they stay in the CLI's own store. Its own OAuth grants live in the OS keyring, or a 0600 file only when the config says so.
- **URLs:** OAuth and discovery URLs must be https, except http on loopback, with no userinfo. A captured redirect must match `redirect_uri` exactly.
- **Child processes:** provider CLIs run from argument lists, never a shell, and without the provider's token variables.

## `@ai-ecoverse/jerk-oauth`

jerk's OAuth core, compiled to WebAssembly for the browser (ESM, wasm-bindgen `web` target). bios uses it for sign-ins in seven, and the vault for refreshes. It does no I/O: the caller does every fetch, opens every popup, and supplies the clock and the random bytes.

```js
import init, { parseConfig, beginSignIn, completeSignIn, parseTokenResponse, parseRefreshResponse,
  refreshRequest, refreshTokenRequest, refreshTokenRequestBytes, scopesSatisfied, shouldBackoff } from '@ai-ecoverse/jerk-oauth';
```

| function | returns |
| --- | --- |
| `parseConfig(tomlOrJson)` | `{ ok, config?, errors? }` for an `[oauth]` block |
| `beginSignIn(config, scope, nowSecs, random32, random16)` | `{ authorizeUrl, state, expectedNonce?, pending }`; keep `pending` in memory |
| `completeSignIn(config, pending, capturedUrl)` | `{ kind: "exchange", url, body }` to POST, or `{ kind: "token", … }` for implicit flows |
| `parseTokenResponse(config, body, nowSecs)` | `{ accessToken, refreshToken?, expiresAt?, scope? }` |
| `refreshTokenRequest(paramsJson)` | `{ url, body }` from `{ token_url, client_id, refresh_token, scope?, resource?, client_secret? }`, no sign-in config needed |
| `refreshTokenRequestBytes(bytes)` | the same, and it zeroes the caller's buffer |
| `parseRefreshResponse(body, nowSecs)` | like `parseTokenResponse`, no config needed |
| `refreshRequest(config, refreshToken)` | `{ url, body }` from a full config |
| `scopesSatisfied(granted, requested)` | boolean |
| `shouldBackoff(lastFailureSecs, nowSecs)` | boolean, a 5-minute window after a failed refresh |

Errors are thrown as `{ kind: "error", code, message }`.
- **Provider errors** (`code: "idp_error"`) also carry `error` and `description`, cut to 128 printable ASCII characters. Use `code` and `error` to avoid showing the provider's free text.
- **The clock:** `nowSecs` must be whole, non-negative seconds (`invalid_clock` otherwise).
- **The random bytes** must come from `crypto.getRandomValues`; jerk only checks they aren't constant.
- **Reproducible:** every release rebuilds the package from its tag and checks it's byte-identical to the published one.

## Development

- `npm run lint`: slicc lint, shellcheck, rustfmt and clippy.
- `npm test`: builds jerk and runs `test/integration/`.
- `npm run test:unit`: the gitignored unit tests in `test/unit/`, with coverage through `cargo-llvm-cov`.
- `npm run build:wasm`: the kernel build, `bin/jerk.wasm`.
- `npm run build:oauth-wasm`: `@ai-ecoverse/jerk-oauth`, into `dist/jerk-oauth/`.

Releases come from `main` through semantic-release.

## License

Apache-2.0
