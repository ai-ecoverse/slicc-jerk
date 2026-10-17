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
jerk mcp:url:<url> create [--scope S] [--no-browser] | get | drop | import-pi
jerk config path [--json] | check | show <provider> | fnox [--proxy] [<provider>...]
jerk intercept --authorize-url U --redirect R [--rewrite m=k=v]... [--pkce] [--exchange --token-url T --client-id C] [--cdp URL]
jerk intercept --from-file F.toml
jerk tab list [--cdp P] [--json]
jerk tab export (--tab ID | --url PATTERN) [--cdp P] > bundle
jerk tab import [--tab ID | --url PATTERN] [--cdp P] < bundle
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

A token already in any of the provider's configured `env` variables (`GH_TOKEN`, `GITHUB_TOKEN`, …) wins over the CLI, the way gh itself behaves. jerk never signs in on its own: when there's no token, `get` exits 3 and names the `create` to run. `get --login` on a terminal runs `create` first.

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
| `claude` | `claude` (Claude Code) | env `ANTHROPIC_API_KEY`, else the token jerk kept from `create` | `create` runs `claude setup-token`, which signs in through the browser and prints a one-year subscription token once; jerk picks it out of the screen output, keeps it in its grant store and returns it from `get`. Run it again with `--force` for a new token. `list` is `claude auth status --json` (Claude Code 2.1.41 or newer). `drop` forgets the kept token and leaves the Claude Code sign-in alone; `claude auth logout` doesn't revoke a `setup-token` token. |
| `cloudflare` | `cf`, then `wrangler`, then browser OAuth | `cf accounts tokens create` (when `-p policies=…`), else `wrangler auth token --json`, else the stored OAuth grant | `create` / `drop` / `list` prefer `cf` (`auth login` / `logout` / `whoami`), then wrangler. `get` via `cf` mints an account-owned API token and needs `CLOUDFLARE_ACCOUNT_ID` plus `-p policies=…`; with the default empty `policies` that step fails and `get` moves on. Wrangler needs 4.57.0+ for `create`/`get` and 4.65.0 for `list`. Browser fallback uses Cloudflare's `cf` OAuth app on `localhost:8877`. Session tokens are treated as expiring after 50 minutes. |
| `aws` | `aws` (CLI v2) | `aws configure export-credentials` | Prints `AccessKeyId`; `lease` maps `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN` from the JSON (session token omitted when absent). Env `AWS_*` wins when `AWS_ACCESS_KEY_ID` is set, including sealed environments with no binary. `-p profile=` / `-p region=` are optional. `create` / `drop` are `aws sso login` / `logout`; `list` / `user` use `sts get-caller-identity`. |
| `gcloud` | `gcloud`, then browser OAuth | `gcloud auth print-access-token`, else the stored OAuth grant | `create` is `gcloud auth login`, `drop` is `gcloud auth revoke`, `list` / `user` are `gcloud auth list` and `gcloud config get-value account`. When the CLI is missing, fall back to the Cloud SDK public desktop client on loopback `:8085` (`cloud-platform` scope). Access tokens are treated as expiring after 55 minutes. |
| `microsoft-graph` | `az`, then Entra OAuth | `az account get-access-token --resource-type ms-graph`, else the stored OAuth grant | Prefer Azure CLI for Graph access tokens. When `az` is missing, fall back to Microsoft identity PKCE (Graph PowerShell public client) so Outlook/SharePoint/Excel skills can share one delegated grant. Env `MICROSOFT_GRAPH_TOKEN` or `EXCEL_GRAPH_TOKEN` wins. `create` / `drop` are `az login` / `logout` (or the OAuth grant). Expiry from the JWT. |
| `adobe-aio` | `aio`, then IMS `client_credentials` | `aio login --bare --no-open`, else a stored/minted S2S grant | Interactive IMS via `aio` (default `-p ctx=cli`). For Adobe Developer Console OAuth Server-to-Server (e.g. `aem.cdn` / edge-functions), pass `-p client_id=… -p client_secret=…` (optional `-p scope=…`; default includes `aem.cdn`). That skips `aio` and POSTs to IMS `token/v3`. Env: `ADOBE_IMS_TOKEN`, then `AEM_EDGE_FUNCTIONS_TOKEN`. Distinct from `firefly`. |
| `firefly` | jerk's own OAuth | IMS `client_credentials` | Server-to-server Firefly Services token. Needs `-p client_id=… -p client_secret=…` (optional `-p org_id=… -p scope=…`). Distinct from `adobe-aio`. |
| `aem` | `aem-ext`, then browser OAuth | env `AEM_TOKEN` / `AEM_API_KEY`, else the stored OAuth grant | When `aem-ext` is on `PATH`, `create` / `list` / `drop` wrap `aem-ext auth key` create, list, and delete (365-day admin API keys, `X-Auth-Token`). `create` prints the key once (`--json` → `/value`, id `/id`) and records the jti for `drop`. Needs `-p org=… -p site=…`. Without `aem-ext`, `create` / `drop` / `get` use the aem-cli loopback OAuth (`authToken`, ~20 minutes). Cookie login (`aem-ext auth login`) stays on the CLI for curl-only use. |
| `fastly` | `fastly` | `fastly profile token` | Env `FASTLY_API_TOKEN` wins. `create` runs `fastly sso`; `list` is `profile list --json`; `drop <profile>` is `profile delete`. Optional `-p profile=…` selects a named profile for get/create. |
| `xai` | jerk's own OAuth | PKCE through a loopback redirect | The sign-in opens in your browser. The grant is kept in the OS keyring. |
| `printful` | `printful` (skill CLI) | `printful auth token` | Store-level private tokens for `api.printful.com`. `PRINTFUL_API_TOKEN` wins when set. `create` is `printful auth login`: pass `-p token=…` to store a Developer Portal token, or omit it to mint via a logged-in `www.printful.com` dashboard tab. `list` / `drop` are `auth status --json` / `auth logout`. |
| `gmail` | jerk's own OAuth | refresh of a Google grant (or `$GWS_REFRESH_TOKEN`) | Loopback PKCE with `access_type=offline`. Defaults to Mozilla Thunderbird's public Google client; override with `GWS_CLIENT_ID` / `GWS_CLIENT_SECRET`. `drop` revokes at Google then clears the grant. |
| `rescuetime` | jerk's own browser capture | the stored API key | `create` opens RescueTime's API key page over CDP; create a key there and jerk keeps it from the form post once RescueTime accepts it. Env `RESCUETIME_API_KEY` wins. Send it as `Authorization: Bearer …` to `/anapi/*`. `drop` forgets the key; delete it on the key page to revoke it. RescueTime's own OAuth needs an app registered with them, so jerk doesn't use it. |
| `slack` | jerk's own browser capture | `SLACK_TOKEN` and `SLACK_D_COOKIE` | `create -p workspace=<team id>` opens `https://app.slack.com/client/<id>` over CDP in a browser already signed in and keeps the session `xoxc` token plus the paired `d` cookie. The id is the `T…` or enterprise `E…` segment of that URL. Slack sends no expiry, so the pair is stored without one and is not refreshed; it stops working when that browser session ends. `drop` forgets the local copy for that workspace. Slack's MCP server is separate: `jerk mcp:url:https://mcp.slack.com/mcp`. |

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
| `store` | `keyring` (default) or `file`, for grants jerk keeps itself; `SLICC_JERK_STORE` sets the default |
| `lease_via` | `get` (default) or `create`, to mint a fresh token per fnox lease |
| `locked` | system files only: later layers can't replace this provider |
| `get`, `list`, `scopes`, `user`, `create`, `drop` | a command, or an array of commands tried in order (see [Fallback chains](#fallback-chains)) |

| command field | meaning |
| --- | --- |
| `run` | the argument list |
| `read` | `get` only: `{ ini = path, key = k }` reads a value from an ini file instead |
| `parse` | `text`, `jwt`, `json:<pointer>`, or `scan:<prefix>` for the last token starting with `<prefix>` in screen output, ignoring ANSI codes and joining a token wrapped over lines |
| `fields` | `get` only: map extra `env` names to `json:<pointer>` values from the same CLI stdout (for multi-value credentials such as AWS) |
| `expiry` | `jwt` to read the expiry from the token, or `json:<pointer>` for an RFC3339 / unix timestamp in the CLI JSON |
| `timeout` | such as `10s` |
| `min_version` | the oldest CLI version that supports this command (exit 4 below it) |
| `interactive` | the command keeps the terminal (sign-ins) |
| `echo` | an `interactive` `create` with `parse`: also show the CLI's screen output while jerk reads it, when stdin is a terminal |
| `keep` | `true` on a `create` with `parse` keeps the token it prints in jerk's grant store, expiring after `max_age`; a `get` or `drop` step with `keep = true` and no other field except `needs` / `unless` returns or forgets it |
| `link` | a `create` that may run from seven over the slicc link |
| `id`, `lookup` | for a `create` that mints a token: where to find its id for revocation |
| `requires` | this step's CLI, in place of the provider's |
| `oauth` | `true` for a `get`, `create` or `drop` step that uses the provider's `[oauth]` block; such a step has no other field except `needs` / `unless` |
| `needs` | param names that must be non-empty for this step to be available |
| `unless` | param names that must be empty for this step to be available |

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
| `client_credentials` | machine grant: POST `token_url` with `client_id` / `client_secret` / `scope` (no browser). `get` re-mints when the cached access token is near expiry |

| `flow` | token grant shape |
| --- | --- |
| `pkce` | authorization code + PKCE (`code_verifier` on the token request) |
| `code` | authorization code without PKCE (optional `client_secret` for public desktop clients) |
| `implicit` | the capture value is the access token (no token endpoint) |
| `exchange` | POST `token_url` with `[oauth.exchange]` (`grant_type` plus a `body` map; `{value}` is the captured param) |
| `client_credentials` | same machine grant as `mode = "client_credentials"` (no browser / capture) |


`create` signs in and keeps the grant: in the OS keyring (service `slicc-jerk`), or with `store = "file"` in a 0600 file. `get` refreshes the token at `token_url` when it's close to expiring. `drop` removes the grant (and POSTs to `revoke_url` when set). On Linux the keyring is the kernel keyring, which doesn't survive a reboot.

Optional oauth fields: `client_secret` (sent on code exchange and refresh), `revoke_url` (best-effort revoke on `drop`), `refresh_env` (env var holding a refresh token that `get` mints from before consulting the stored grant). `client_id` and `client_secret` may use `${NAME:-default}` expansion from the process environment.

`jerk aem create -p org=… -p site=…` prefers `aem-ext auth key create --register --json` when that CLI is installed (long-lived admin API key). Without `aem-ext`, it uses the aem-cli loopback login (`client_id=aem-cli`, redirect `/.aem/cli/login/ack`). After you click Send, jerk stores the short-lived admin JWT (`authToken`) for `api.aem.live` as `AEM_TOKEN` (`X-Auth-Token` / `Authorization: token …`). The login host defaults to `admin.hlx.page` (Helix 6 sites still use that login endpoint today); override with `-p login_host=…` when `api.aem.live` returns `authToken` itself. Body capture CORS allow-list is the provider's `cors_origins`; any `{placeholder}` in `authorize_url` beyond the built-in OAuth vars must be supplied with `-p`.

`jerk garmin create` opens Garmin SSO over CDP (`intercept`), captures `ticket` from the `connect.garmin.com/modern` redirect, and exchanges it at `diauth.garmin.com` with grant type `…/oauth/grant/service_ticket` (`flow = "exchange"`). The access token is `GARMIN_TOKEN` for `connectapi.garmin.com`.

`jerk rescuetime create` uses `capture = { at = "network", from = "body", url = "https://www.rescuetime.com/anapi/setup_submit", param = "candidate" }`. With `from = "body"`, jerk waits for the response to a request matching `url`, and only when the status is below 400 takes `param` from its form or JSON body as the token. `capture.stage = "request"` reads that body on the way out instead, before a response exists, and only applies to a body capture. `capture.fields` takes more values from the same paused request: `cookie:<name>`, `header:<name>`, `body:<name>` or `query:<name>`. A cookie comes from the `Cookie` header, or from the browser cookie jar when that header is missing. Each field name must be listed in `env` and must not be the first name there. `{placeholders}` in an intercept `authorize_url` or `capture.url`, aside from the built-in OAuth ones, must be declared in `params` and come from `-p`. Built-in names in `capture.url`, such as `{client_id}`, are filled with the same values as the login URL. `config check` rejects a name that is not a param, rejects `{code_challenge}` unless the flow is `pkce`, and rejects `{client_secret}` unless `client_secret` is set. A provider with `capture.fields` stays multi-value when a secondary variable is unset, so `lease` and `sync` do not copy the token into that name. `get --json` masks `credentials` unless `--reveal`. The stored grant is keyed by `user` when that is set, otherwise by `workspace`, otherwise `default`.

`jerk slack create -p workspace=T0123456789` opens that workspace in a new tab of the signed-in browser (the tab shares the browser's cookies) and keeps `token` from a `https://*.slack.com/api/*` form body as `SLACK_TOKEN`, plus the `d` cookie as `SLACK_D_COOKIE`. `get` prints both; a pipe masks each value unless `--reveal`. There is no `list`. Enterprise Grid sign-in that needs a Slack app or an admin approval is out of scope; a workspace that is already signed in, including an enterprise workspace, uses this same capture. Slack's own MCP server needs a registered app and does not offer dynamic client registration, so use `jerk mcp:url:https://mcp.slack.com/mcp` for that.

For `intercept`, the browser is the one in `--cdp`, otherwise `SLICC_CDP_URL`, otherwise a running browser from `slicc browsers --json` (the one with seven open first), otherwise exit 4. State is only checked when `authorize_url` contains `{state}`.

### Fallback chains

Each command can be an array of steps, tried in order. The built-in `cloudflare` provider prefers `cf`, then `wrangler`, then jerk's own OAuth sign-in (`jerk config show cloudflare`). A custom chain looks the same:

```toml
[providers.flare]
name = "Cloudflare"
env = ["CLOUDFLARE_API_TOKEN"]
domains = ["api.cloudflare.com"]
max_age = "50m"
params = { name = "jerk", policies = "" }

[[providers.flare.get]]
requires = "cf"
run = ["cf", "accounts", "tokens", "create", "--quiet", "--name", "{name}", "--policies", "{policies}"]
parse = "json:/value"

[[providers.flare.get]]
requires = "wrangler"
run = ["wrangler", "auth", "token", "--json"]
parse = "json:/token"
min_version = "4.57.0"

[[providers.flare.get]]
oauth = true

[[providers.flare.create]]
requires = "cf"
run = ["cf", "auth", "login", "--scopes", "{scope}"]
interactive = true

[[providers.flare.create]]
requires = "wrangler"
run = ["wrangler", "login", "--scopes", "{scope}"]
interactive = true
min_version = "4.57.0"

[[providers.flare.create]]
oauth = true

[providers.flare.oauth]
mode = "loopback"
flow = "pkce"
authorize_url = "https://dash.cloudflare.com/oauth2/auth?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&state={state}&code_challenge={code_challenge}&code_challenge_method=S256"
token_url = "https://dash.cloudflare.com/oauth2/token"
client_id = "cbca97e7-c331-4cdd-8fd8-e25a451b98bf"
redirect_uri = "http://localhost:8877/oauth/callback"
scope = "account:read user:read workers:write offline_access"
capture = { at = "request", from = "query", param = "code" }
```

Set `CLOUDFLARE_ACCOUNT_ID` and `-p policies='[…]'` (account-scoped resources) when you want `get` to mint through `cf`. With the default empty `policies`, `--policies` is omitted, that step fails, and `get` moves on.

- A step is **unavailable** when its CLI isn't on `PATH`, or its `min_version` probe fails or reports an older version. Unavailable steps are always skipped.
- `get`, `list`, `scopes` and `user` also move on when a step fails: a non-zero exit, an empty or unparseable token, or a timeout.
- `create` and `drop` run the first available step, and its result is final. jerk never starts a second sign-in.
- When every `get` step fails, `get` exits 3 if some step ran, or 4 if none was available. The message lists each step and why it failed, but never a token or the CLI's output.
- A token in any of the provider's configured `env` variables still wins over every step. `get --login`, `lease`, `sync`, `--via-link` and the "already signed in" check in `create` use the same chains, and `link` is set on each `create` step.
- An `[oauth]` block that no step names keeps its old meaning: `get` uses it in place of any CLI steps, and `create` and `drop` use it only when they have no command. Add `{ oauth = true }` to put it in a chain.

jerk's own OAuth grant is separate from the CLIs' sign-ins. `wrangler login` doesn't give jerk a grant, and jerk's `[oauth]` sign-in doesn't sign wrangler in, so a chain can hold two different accounts. `jerk config show` prints the chain, and `jerk list` shows each step's CLI, such as `cf (missing), wrangler, oauth`.

### MCP servers

Every server with a `url` in pi's `~/.pi/agent/mcp.json` (or `PI_MCP_CONFIG`) is a provider named `mcp:<server>`, and its token's variable is `MCP_<SERVER>_TOKEN`. Server names starting with `url:` are skipped.

```sh
jerk mcp:linear create            # discovers the authorization server, registers a client, signs in
jerk mcp:linear get --reveal --json
jerk mcp:linear import-pi         # copy an existing sign-in from pi's mcp-auth.json
```

A server can also be named by its URL alone: `mcp:url:<url>`. It takes the same commands and flags, and `jerk config show` works for it too.

```sh
jerk mcp:url:https://mcp.example.com/mcp create
jerk mcp:url:HTTPS://MCP.Example.com:443/mcp get   # the same provider
```

- **Canonical form.** The URL must be `https`, or `http` on loopback (`localhost`, `127.0.0.1`, `::1`). Userinfo, a query or a fragment exit 2, and the error never repeats the URL. The scheme and host are lower-cased, the default port is dropped and an empty path becomes `/`; the rest of the path is kept, including a trailing slash. jerk prints the canonical id, `mcp:url:https://mcp.example.com/mcp`.
- **Same URL as an `mcp.json` entry.** When an entry's `url` has the same canonical form, `mcp:url:` is that entry: its `oauth` fields, its variable and its grant. If several entries share the URL, the first by name wins, and `config show` lists the others.
- **URL only.** Otherwise there are no `oauth` fields: the client is mcpcat's metadata document, else dynamic registration, and the callback is the default one. There is no variable, so `get` never reads the environment, and `get --json` reports `"env": []`. The grant goes in the OS keyring under `slicc-jerk` / `mcp:url:<canonical url>/url`, which can't clash with `mcp:<server>`. `import-pi` looks the URL up in `mcp-auth.json` (pi's legacy URL key, else a single `<server>|<url>` key) and exits 5 when several pi servers use it.
- `jerk list` doesn't show URL-only ids, because they aren't configured.

`create` follows the MCP authorization spec:
- it finds the authorization server through protected-resource metadata (RFC 9728) and its metadata (RFC 8414);
- it registers through mcpcat's client metadata document, or dynamic registration, or uses the `clientId` from `mcp.json`;
- it signs in with PKCE, sending the server as `resource` (RFC 8707).

`--no-browser` prints the URL and reads the redirected URL from stdin. pi's `oauth` fields in `mcp.json` are honoured. mcpcat uses these providers whenever jerk is installed.

## Moving a tab's session

`jerk tab` copies one tab's signed-in state (its cookies, `localStorage` and `sessionStorage`) into another tab, in the same browser or in a different one. It works like slicc's `playwright teleport`, but you choose the tab, it doesn't wait for a URL pattern, and the state goes through a file or a pipe.

```sh
jerk tab list --cdp 9222                                        # tab ids, URLs and titles
jerk tab export --url mail.example.com --cdp 9222 > tab.bundle
jerk tab import --cdp 9333 < tab.bundle                         # a new tab in another browser
jerk tab export --tab 3F2A --cdp 9222 | jerk tab import --tab 9C1B --cdp 9222
```

- **Picking the tab:** `--tab` takes a target id or a unique prefix of one. `--url` matches part of the URL, or the whole URL when the pattern contains `*`. If more than one tab matches, jerk lists them and exits 2.
- **The browser:** `--cdp` takes a port (`9222` becomes `http://127.0.0.1:9222`), an `http(s)` DevTools URL or a `ws(s)` URL. Without it, jerk uses `SLICC_CDP_URL`, then `slicc browsers`. In seven, `SLICC_CDP_URL` is the kernel's browser (see [In SLICC](#in-slicc)), and the kernel build has no TLS, so `wss://` and `https://` don't work there. It works with Chromium browsers (Chrome, Edge, Brave, Chromium).
- **`export`** writes the state as one base64 line: the cookies Chrome sends to the page and its frames (HttpOnly included), plus the top frame's storage. Base64 isn't encryption, so the bundle is as sensitive as a password. jerk won't print it to a terminal.
- **`import`** opens a new tab, or uses the one `--tab` or `--url` picks, and sets the cookies. Host-only and `__Host-` cookies stay host-only. It writes the storage before the page's own scripts run, opens the exported URL and prints the tab id. If the page redirects to another origin (a sign-in page, say), the storage can't be written there, so jerk exits 1.
- **Not carried:** IndexedDB, Cache Storage, service workers, cookies of other sites (an identity provider's, for example), and anything tied to the device, such as client certificates or device-bound session credentials. Some sites also tie a session to its IP address or user agent.
- jerk never opens or exports a seven tab. It never reads seven's cookies, and on import it leaves them out of a bundle that has any, so the rest still get set.

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
- **`tab`** drives the browser seven runs in, through the kernel's CDP endpoint in `SLICC_CDP_URL`, whether the slicc extension or the slicc CLI provides it. `--cdp 9222` names the same endpoint. A bundle exported in seven holds that tab's cookies and storage in plain text, so keep it out of shared folders.
- **`<provider> list`, `lease`, `sync` and `intercept`** exit 5.

All of this goes through `/dev/sealed`.

On the machine side, the slicc CLI runs jerk with `--via-link`:
- `create --via-link` prints progress lines and a final status as newline-delimited JSON, and never prompts. It refuses `--force`, minted tokens, and sign-ins that need a terminal.
- `lease --via-link` prints the lease, then a status.
- `drop --lease --via-link` prints a status. A CLI sign-out is never run over the link.

## Security

- **Printing tokens:** jerk prints a token only to a terminal or with `--reveal`, and never logs one. Errors never repeat tokens, codes, verifiers, `state`, config values or response bodies.
- **Tokens at rest:** jerk stores no access tokens of CLI providers; they stay in the CLI's own store, except a token a `create` with `keep = true` prints once. Its own OAuth grants live in the OS keyring, or a 0600 file only when the config says so.
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
