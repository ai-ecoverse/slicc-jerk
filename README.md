# slicc-jerk

`jerk` gets, lists, creates and drops OAuth tokens on macOS, Linux and Windows, and inside [SLICC](https://github.com/ai-ecoverse/slicc). It asks each provider's own CLI (`gh`, `npm`, `wrangler`, `aio`, …) for its token, or runs the OAuth flow itself when there is no CLI. Adding a provider is configuration, not code.

The name is the ice-cream kind: a soda jerk serves you what you asked for.

- [Install](#install) · [Quick start](#quick-start) · [Commands](#commands) · [Providers](#providers) · [Configuring providers](#configuring-providers)
- [MCP servers](#mcp-servers) · [Moving a tab's session](#moving-a-tabs-session) · [fnox](#fnox) · [In SLICC](#in-slicc) · [Security](#security) · [`@ai-ecoverse/jerk-oauth`](#ai-ecoversejerk-oauth) · [Related projects](#related-projects)

## Install

```sh
curl -fsSL https://get.sliccy.ai/jerk | sh
```

The installer puts `jerk` in `~/.local/bin` (`INSTALL_DIR` or `--dir` to change it, `VERSION` or `--version` to pin one), checks it against the release's `SHA256SUMS`, and never uses `sudo`. Pass options with `sh -s -- --help`. The same script is a [release asset](https://github.com/ai-ecoverse/slicc-jerk/releases/latest/download/install.sh) if get.sliccy.ai is unreachable.

- **npm:** `npm install -g @ai-ecoverse/slicc-jerk`. A small Node launcher picks the platform package `@ai-ecoverse/slicc-jerk-<os>-<arch>`.
- **slicc:** the [slicc CLI](https://github.com/ai-ecoverse/slicc-cli) installs jerk with it, or with `slicc install jerk`.
- **Binaries:** [GitHub releases](https://github.com/ai-ecoverse/slicc-jerk/releases) carry `jerk-<os>-<arch>` with `SHA256SUMS` and build provenance. Linux builds are static (musl). macOS builds are signed with a Developer ID (`ai.sliccy.jerk`) and notarized. Windows builds are unsigned, so SmartScreen may warn on first run.

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
jerk <provider> drop   [<id>] [--scope S] [--yes]
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
jerk upgrade [--check] [--force]
```

`--config <file>` (or `SLICC_JERK_CONFIG`) adds a providers file on top of the others. `-p k=v` sets a provider parameter, for example `-p host=github.example.com`.

`jerk upgrade`, or `jerk update`, replaces a jerk from install.sh or the releases page with the latest release, in place. It fetches over https only, and the download must match the release's `SHA256SUMS` and answer `--version` with the release version before it's renamed over the old one. `--check` only reports whether a newer release exists. A binary or directory jerk can't write is left alone, a development build needs `--force`, and a copy installed with npm, cargo or `slicc install jerk` is left to that tool, with the command to run. The kernel build exits 5.

`get` prints the token to a terminal, or with `--reveal`. In a pipe it prints a masked form such as `gho_…1234`, so it doubles as a status check. `--json` prints `provider`, `source`, `masked`, `expires_at` and `scopes`, plus `token` only with `--reveal`; `--refresh` prints a status line, never the token. For an env token, `--scope` checks the JWT's `scp` or `scope` claim when it has one (the signature isn't verified), otherwise the provider's `scopes` command.

A token already in one of the provider's `env` variables (`GH_TOKEN`, `GITHUB_TOKEN`, …) wins over the CLI, the way gh itself behaves. jerk never signs in on its own: without a token, `get` exits 3 and names the `create` to run, and `get --login` on a terminal runs it first.

| exit | meaning |
| --- | --- |
| 0 | ok |
| 1 | error |
| 2 | usage, config or policy refusal |
| 3 | needs an interactive sign-in, or the token doesn't cover `--scope` |
| 4 | not available here: the CLI is missing or too old, no browser, no linked slicc CLI |
| 5 | unsupported for this provider or runtime |

## Providers

Built in: `github`, `npm`, `claude`, `openai`, `huggingface`, `cloudflare`, `aws`, `gcloud`, `microsoft-graph`, `adobe-aio`, `firefly`, `aem`, `fastly`, `xai`, `linkedin`, `printful`, `gmail`, `google`, `garmin`, `rescuetime`, `slack`, `kagi`, `elevenlabs` and `openrouter`. Every [MCP server](#mcp-servers) is one too. `jerk config show <provider>` prints the definition in effect.

<details>
<summary>What each built-in provider runs</summary>

| id | backed by | `get` | notes |
| --- | --- | --- | --- |
| `github` | `gh` | `gh auth token` | `--scope` understands GitHub's implied scopes (`repo` covers `public_repo`). Sign-out is `gh auth logout`. |
| `npm` | `npm` | `//registry.npmjs.org/:_authToken` from `$NPM_CONFIG_USERCONFIG` or `~/.npmrc` | `npm config get` won't print auth keys, so jerk reads the file. `create` mints a granular token (npm 11.6.3 or newer) and records only its id, so `drop <id>` can revoke it. |
| `claude` | `claude` (Claude Code) | env `ANTHROPIC_API_KEY`, else the token jerk kept from `create` | `create` runs `claude setup-token`, which signs in through the browser and prints a one-year subscription token once; jerk picks it out of the screen output and keeps it. `--force` gets a new one. `list` is `claude auth status --json` (Claude Code 2.1.41 or newer). `drop` forgets the kept token and leaves the Claude Code sign-in alone; `claude auth logout` doesn't revoke a `setup-token` token. |
| `openai` | `codex` (Codex CLI) | `OPENAI_API_KEY` from `${CODEX_HOME:-~/.codex}/auth.json` | Env `OPENAI_API_KEY` wins, and `get` needs no `codex` to read the file. A ChatGPT sign-in's `tokens.access_token` is left out: it's for `chatgpt.com`'s Codex backend, not `api.openai.com`, so a ChatGPT-only sign-in with no API key exits 3. `create` is `codex login`, whose ChatGPT sign-in stores an API key only when OpenAI's token exchange issues one, so if `get` still exits 3, run `codex login --with-api-key` yourself. `drop` is `codex logout`. There's no `list`, because `codex login status` writes only to stderr. Credentials Codex keeps in the OS keyring (`cli_auth_credentials_store`) aren't read. |
| `huggingface` | `hf` | the token file at `$HF_TOKEN_PATH`, else `$HF_HOME/token`, else `${XDG_CACHE_HOME:-~/.cache}/huggingface/token` | The same order as `huggingface_hub`. Env `HF_TOKEN`, then `HUGGING_FACE_HUB_TOKEN`, wins. `create` is `hf auth login`, `drop [<name>]` is `hf auth logout [--token-name <name>]`, and `user` is `user` (or `name`, before huggingface_hub's CLI rewrite) from `hf auth whoami --format json`. |
| `cloudflare` | `cf`, then `wrangler`, then browser OAuth | `cf accounts tokens create` (when `-p policies=…`), else `wrangler auth token --json`, else the stored OAuth grant | `create` / `drop` / `list` prefer `cf` (`auth login` / `logout` / `whoami`), then wrangler. `get` via `cf` mints an account-owned API token and needs `CLOUDFLARE_ACCOUNT_ID` plus `-p policies=…`. Wrangler needs 4.57.0+ for `create`/`get` and 4.65.0 for `list`. The browser fallback uses Cloudflare's `cf` OAuth app on `localhost:8877`. Session tokens are treated as expiring after 50 minutes. |
| `aws` | `aws` (CLI v2) | `aws configure export-credentials` | Prints `AccessKeyId`; `lease` maps `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN` (the last only when present). Env `AWS_*` wins when `AWS_ACCESS_KEY_ID` is set, even with no binary. `-p profile=` / `-p region=` are optional. `create` / `drop` are `aws sso login` / `logout`; `list` / `user` use `sts get-caller-identity`. |
| `gcloud` | `gcloud`, then browser OAuth | `gcloud auth print-access-token`, else the stored OAuth grant | `create` is `gcloud auth login`, `drop` is `gcloud auth revoke`, `list` / `user` are `gcloud auth list` and `gcloud config get-value account`. Without the CLI, jerk uses the Cloud SDK public desktop client on loopback `:8085` (`cloud-platform` scope). Access tokens are treated as expiring after 55 minutes. |
| `microsoft-graph` | `az`, then Entra OAuth | `az account get-access-token --resource-type ms-graph`, else the stored OAuth grant | Without `az`, jerk uses Microsoft identity PKCE (Microsoft Graph Command Line Tools, `14d82eec-204b-4c2f-b7e8-296a70dab67e`), so Outlook, SharePoint, Excel and Teams skills share one delegated grant. The default scope is `offline_access openid profile User.Read Mail.ReadWrite Mail.Send Calendars.ReadWrite Files.ReadWrite.All Sites.ReadWrite.All`. The Azure CLI's app only gets a fixed set of Graph scopes, so `--scope` (or `-p scope=…`) skips `az` on `get`, `create` and `drop` and uses the Entra grant instead. For Teams: `jerk microsoft-graph create --scope "offline_access Chat.ReadWrite ChannelMessage.Read.All Team.ReadBasic.All Channel.ReadBasic.All"`, then `jerk microsoft-graph get --scope Chat.ReadWrite`, which exits 3 when the grant doesn't cover it; `drop --scope Chat.ReadWrite` forgets that grant. Env `MICROSOFT_GRAPH_TOKEN`, `EXCEL_GRAPH_TOKEN` or `TEAMS_GRAPH_TOKEN` wins. `create` / `drop` are `az login` / `logout` (or the OAuth grant). Expiry from the JWT. |
| `adobe-aio` | `aio`, then IMS `client_credentials` | `aio login --bare --no-open`, else a stored or minted S2S grant | Interactive IMS via `aio` (default `-p ctx=cli`). For Adobe Developer Console OAuth Server-to-Server (e.g. `aem.cdn` / edge functions), pass `-p client_id=… -p client_secret=…` (optional `-p scope=…`; the default includes `aem.cdn`), which skips `aio` and POSTs to IMS `token/v3`. Env `ADOBE_IMS_TOKEN`, then `AEM_EDGE_FUNCTIONS_TOKEN`. |
| `firefly` | jerk's own OAuth | IMS `client_credentials` | Server-to-server Firefly Services token. Needs `-p client_id=… -p client_secret=…` (optional `-p org_id=… -p scope=…`). |
| `aem` | `aem-ext`, then browser OAuth | env `AEM_TOKEN` / `AEM_API_KEY`, else the stored OAuth grant | Needs `-p org=… -p site=…`. With `aem-ext` on `PATH`, `create` / `list` / `drop` wrap `aem-ext auth key` (`create --register --json`, list, delete): 365-day admin API keys sent as `X-Auth-Token`, printed once, with the jti recorded for `drop`. Without it, jerk uses the aem-cli loopback login (`client_id=aem-cli`, redirect `/.aem/cli/login/ack`) and keeps the ~20-minute admin JWT (`authToken`) for `api.aem.live` as `AEM_TOKEN`. The login host defaults to `admin.hlx.page`; `-p login_host=…` overrides it. |
| `fastly` | `fastly` | `fastly profile token` | Env `FASTLY_API_TOKEN` wins. `create` runs `fastly sso`; `list` is `profile list --json`; `drop <profile>` is `profile delete`. `-p profile=…` picks a named profile. |
| `xai` | jerk's own OAuth | PKCE through a loopback redirect | The sign-in opens in your browser. The grant is kept in the OS keyring. |
| `linkedin` | jerk's own OAuth | the stored OAuth grant | Authorization code caught in a browser over CDP. Needs your LinkedIn app's `-p client_id=… -p client_secret=…`. |
| `printful` | `printful` (skill CLI) | `printful auth token` | Store-level private tokens for `api.printful.com`. `PRINTFUL_API_TOKEN` wins. `create` is `printful auth login`: `-p token=…` stores a Developer Portal token, otherwise it mints one through a signed-in `www.printful.com` dashboard tab. `list` / `drop` are `auth status --json` / `auth logout`. |
| `gmail` | jerk's own OAuth | refresh of a Google grant (or `$GWS_REFRESH_TOKEN`) | Loopback PKCE with `access_type=offline`. Defaults to Mozilla Thunderbird's public Google client; override with `GWS_CLIENT_ID` / `GWS_CLIENT_SECRET`. `drop` revokes at Google, then clears the grant. |
| `google` | jerk's own OAuth | refresh of one Google grant | One grant for Calendar, Drive, Sheets, Docs and other Google APIs, picked with `--scope` or `-p scope=…`: `https://www.googleapis.com/auth/calendar`, `…/auth/drive`, `…/auth/spreadsheets`, `…/auth/documents`. A bare `create` asks only for `openid` and the email scope. On an existing grant, `create --scope` asks Google for the old scopes plus the new ones (`include_granted_scopes=true`), so the grant keeps covering both; `--force` asks for only what you name. The token is `GOOGLE_ACCESS_TOKEN`; `gmail` keeps its own grant. Loopback PKCE with `access_type=offline` and the same default client as `gmail` (Mozilla Thunderbird's), which Google may refuse for Drive, Sheets and Docs: set `GWS_CLIENT_ID` / `GWS_CLIENT_SECRET` to your own desktop client then. `drop` revokes at Google, then clears the grant. |
| `garmin` | jerk's own browser capture | the stored OAuth grant | Opens Garmin SSO over CDP, catches `ticket` from the `connect.garmin.com/modern` redirect and exchanges it at `diauth.garmin.com` (`…/oauth/grant/service_ticket`). The token is `GARMIN_TOKEN` for `connectapi.garmin.com`. |
| `rescuetime` | jerk's own browser capture | the stored API key | `create` opens RescueTime's API key page over CDP; create a key there and jerk keeps it from the form post once RescueTime accepts it. Env `RESCUETIME_API_KEY` wins. Send it as `Authorization: Bearer …` to `/anapi/*`. `drop` forgets the key; delete it on the key page to revoke it. |
| `openrouter` | jerk's own OAuth | the stored API key | Loopback PKCE (S256) through `openrouter.ai/auth` with a `callback_url` on `localhost:56124`; jerk posts the code as JSON to `/api/v1/auth/keys` and keeps the returned `key` as `OPENROUTER_API_KEY` for `openrouter.ai`. No app registration: `client_id = "slicc-jerk"` is only the prefilled key label. The key has no expiry and no refresh. Env `OPENROUTER_API_KEY` wins. `drop` forgets the key; revoke it on openrouter.ai's keys page. |
| `slack` | jerk's own browser capture | `SLACK_TOKEN` and `SLACK_D_COOKIE` | `create -p workspace=<T… or E… id>` opens `https://app.slack.com/client/<id>` in a new tab of a browser that's already signed in, and keeps the session `xoxc` token plus the paired `d` cookie. There's no expiry and no refresh: the pair stops working when that browser session ends. `drop` forgets the local copy. Sign-ins that need a Slack app or admin approval are out of scope. For Slack's MCP server, use `jerk mcp:url:https://mcp.slack.com/mcp`. |
| `kagi` | a pasted key | env `KAGI_API_KEY`, else the key jerk kept from `create` | `create` prints and opens `https://kagi.com/api/keys`, then reads the key you paste without echoing it, or from stdin when that isn't a terminal (`printf %s "$KEY" \| jerk kagi create`). `--force` replaces it. `drop` forgets the key; revoke it on the key page. |
| `elevenlabs` | a pasted key | env `ELEVENLABS_API_KEY`, then `XI_API_KEY`, else the key jerk kept from `create` | `create` prints and opens `https://elevenlabs.io/app/settings/api-keys` and keeps the key you paste or pipe in, like `kagi`. Send it as `xi-api-key: …` to `api.elevenlabs.io`. `drop` forgets the key; revoke it on the API keys page. |

</details>

## Configuring providers

Providers are TOML tables in layers, from lowest to highest priority. A provider in a later layer replaces the earlier one as a whole, except a system provider with `locked = true`. `jerk config check` validates every layer, and `jerk config path` shows where each one lives.

| layer | macOS | Linux | Windows |
| --- | --- | --- | --- |
| built-in | compiled into jerk | same | same |
| system | `/Library/Application Support/slicc-jerk/providers.d/*.toml` | `/etc/slicc-jerk/providers.d/*.toml` | `%ProgramData%\slicc-jerk\providers.d\*.toml` |
| user | `$XDG_CONFIG_HOME/slicc-jerk/providers.toml`, default `~/.config/slicc-jerk/providers.toml` | same | `%APPDATA%\slicc-jerk\providers.toml` |
| explicit | `--config <file>` or `SLICC_JERK_CONFIG` | same | same |

`SLICC_JERK_SYSTEM_DIR` overrides the system directory everywhere. jerk keeps its state (leases, minted token ids, file-stored grants) in `~/Library/Application Support/slicc-jerk` on macOS, `$XDG_STATE_HOME/slicc-jerk` (default `~/.local/state/slicc-jerk`) on Linux and `%LOCALAPPDATA%\slicc-jerk` on Windows; `SLICC_JERK_STATE_DIR` overrides it. On macOS, `jerk config check` warns while `/etc/slicc-jerk/providers.d`, the old system directory, still exists.

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

Commands are argument lists, never shell strings, and the CLI runs without the provider's token variables in its environment. `{name}` placeholders come from `params`, or from `scope`, `id`, `name`, `expires` and `user`. An argument whose placeholder is empty is dropped, together with the flag before it.

<details>
<summary>Provider and command fields</summary>

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
| `read` | `get` only: read the token from a file instead. `{ ini = path, key = k }` takes a key from an ini file, `{ file = path }` the trimmed contents, and `{ json = path, pointer = "/…" }` a string from a JSON file. A missing file, key or value, or an empty or null one, moves the chain on. Paths expand `${NAME:-default}`, nested too, and a leading `~` |
| `parse` | `text`, `jwt`, `json:<pointer>`, or `scan:<prefix>` for the last token starting with `<prefix>` in screen output, ignoring ANSI codes and joining a token wrapped over lines |
| `fields` | `get` only: map extra `env` names to `json:<pointer>` values from the same CLI stdout (for multi-value credentials such as AWS) |
| `expiry` | `jwt` to read the expiry from the token, or `json:<pointer>` for an RFC3339 / unix timestamp in the CLI JSON |
| `timeout` | such as `10s` |
| `min_version` | the oldest CLI version that supports this command (exit 4 below it) |
| `interactive` | the command keeps the terminal (sign-ins) |
| `echo` | an `interactive` `create` with `parse`: also show the CLI's screen output while jerk reads it, when stdin is a terminal |
| `ask` | a `create` with `keep = true` and no other field except `needs` / `unless`: the http(s) page where the user makes a key; jerk prints it, opens it when stdin is a terminal, reads the pasted key without echo (or the first line of stdin otherwise, exit 3 when empty) and keeps it |
| `keep` | `true` on a `create` with `parse` keeps the token it prints in jerk's grant store, expiring after `max_age`; a `get` or `drop` step with `keep = true` and no other field except `needs` / `unless` returns or forgets it |
| `link` | a `create` that may run from seven over the slicc link |
| `id`, `lookup` | for a `create` that mints a token: where to find its id for revocation |
| `requires` | this step's CLI, in place of the provider's |
| `oauth` | `true` for a `get`, `create` or `drop` step that uses the provider's `[oauth]` block; such a step has no other field except `needs` / `unless` |
| `needs` | param names that must be non-empty for this step to be available |
| `unless` | param names that must be empty for this step to be available |

</details>

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

`create` signs in and keeps the grant in the OS keyring (service `slicc-jerk`), or with `store = "file"` in a 0600 file. On Linux that's the kernel keyring, which doesn't survive a reboot. `get` refreshes the token at `token_url` when it's close to expiring, and `drop` removes the grant.

<details>
<summary>Modes, flows and optional fields</summary>

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
| `exchange` | POST `token_url` with `[oauth.exchange]` (`grant_type` plus a `body` map) |
| `client_credentials` | same machine grant as `mode = "client_credentials"` (no browser / capture) |

`[oauth.exchange]` shapes the token request. With `flow = "exchange"` it needs `grant_type`, and jerk sends `grant_type` and `client_id` ahead of `body`. With `pkce` or `code` it replaces the standard code exchange: `grant_type` is optional, and without it jerk sends exactly `body`. In `body`, `{value}` is the captured param, `{code_verifier}` the PKCE verifier, and `{client_id}`, `{redirect_uri}` and `{scope}` work too. `format = "json"` posts a JSON object instead of a form (the default, `"form"`). `token_field` names the response field that holds the token (default `access_token`); with it set, `token_type` may be missing. `config check` rejects `[oauth.exchange]` on `implicit`, `client_credentials` and `mode = "device"`, which never use it.

Optional fields: `client_secret` (sent on code exchange and refresh), `revoke_url` (best-effort revoke on `drop`), and `refresh_env` (an env var holding a refresh token that `get` mints from before it looks at the stored grant). `client_id` and `client_secret` may use `${NAME:-default}` expansion from the environment.

For `intercept`, the browser is the one in `--cdp`, otherwise `SLICC_CDP_URL`, otherwise a running browser from `slicc browsers --json` (the one with seven open first), otherwise exit 4. State is only checked when `authorize_url` contains `{state}`.

</details>

<details>
<summary>Capturing from a request or response body</summary>

`rescuetime` uses `capture = { at = "network", from = "body", url = "https://www.rescuetime.com/anapi/setup_submit", param = "candidate" }`.

- With `from = "body"`, jerk waits for the response to a request matching `url`, and only when the status is below 400 takes `param` from its form or JSON body as the token. `capture.stage = "request"` reads that body on the way out instead, before a response exists.
- `capture.fields` takes more values from the same paused request: `cookie:<name>`, `header:<name>`, `body:<name>` or `query:<name>`. A cookie comes from the `Cookie` header, or from the browser cookie jar when that header is missing. Each field name must be in `env`, but not first.
- A provider with `capture.fields` stays multi-value when a secondary variable is unset, so `lease` and `sync` don't copy the token into that name. `get --json` masks `credentials` unless `--reveal`.
- The provider's `cors_origins` is the CORS allow-list for a body capture.
- The stored grant is keyed by `user` when that is set, otherwise by `workspace`, otherwise `default`.
- `{placeholders}` in an intercept `authorize_url` or `capture.url`, other than the built-in OAuth ones, must be declared in `params` and come from `-p`. Built-in names in `capture.url`, such as `{client_id}`, get the same values as in the login URL. `config check` rejects a name that isn't a param, `{code_challenge}` unless the flow is `pkce`, and `{client_secret}` unless `client_secret` is set.

</details>

### Fallback chains

Each command can be an array of steps, tried in order. The built-in `cloudflare` provider prefers `cf`, then `wrangler`, then jerk's own OAuth sign-in.

```toml
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
```

`jerk config show cloudflare` prints the full chain, including `create` and the `[oauth]` block.

- A step is **unavailable** when its CLI isn't on `PATH`, or its `min_version` probe fails or reports an older version. Unavailable steps are always skipped.
- `get`, `list`, `scopes` and `user` also move on when a step fails: a non-zero exit, an empty or unparseable token, or a timeout.
- `create` and `drop` run the first available step, and its result is final. jerk never starts a second sign-in.
- When every `get` step fails, `get` exits 3 if some step ran, or 4 if none was available. The message lists each step and why it failed, but never a token or the CLI's output.
- A token in any of the provider's `env` variables still wins over every step. `get --login`, `lease`, `sync`, `--via-link` and the "already signed in" check in `create` use the same chains.
- An `[oauth]` block that no step names is used by `get` in place of any CLI steps, and by `create` and `drop` only when they have no command. Add `{ oauth = true }` to put it in a chain.
- jerk's own OAuth grant is separate from the CLIs' sign-ins, so a chain can hold two different accounts. `jerk list` shows each step's CLI, such as `cf (missing), wrangler, oauth`.

## MCP servers

Every server with a `url` in [pi](https://github.com/earendil-works/pi)'s `~/.pi/agent/mcp.json` (or `PI_MCP_CONFIG`) is a provider named `mcp:<server>`, with its token in `MCP_<SERVER>_TOKEN`. Any other server can be named by its URL, `mcp:url:<url>`. [mcpcat](https://github.com/ai-ecoverse/mcpcat) uses these providers whenever jerk is installed.

```sh
jerk mcp:linear create            # discovers the authorization server, registers a client, signs in
jerk mcp:linear get --reveal --json
jerk mcp:linear import-pi         # copy an existing sign-in from pi's mcp-auth.json
jerk mcp:url:https://mcp.example.com/mcp create
```

`create` follows the MCP authorization spec: it finds the authorization server through RFC 9728 and RFC 8414 metadata, registers through mcpcat's client metadata document or dynamic registration (or uses the `clientId` from `mcp.json`), and signs in with PKCE, sending the server as `resource` (RFC 8707). `--no-browser` prints the URL and reads the redirected URL from stdin. pi's `oauth` fields in `mcp.json` are honoured.

<details>
<summary>How <code>mcp:url:</code> ids work</summary>

- **Canonical form.** The URL must be `https`, or `http` on loopback (`localhost`, `127.0.0.1`, `::1`). Userinfo, a query or a fragment exit 2, and the error never repeats the URL. The scheme and host are lower-cased, the default port is dropped and an empty path becomes `/`; the rest of the path is kept, including a trailing slash. So `mcp:url:HTTPS://MCP.Example.com:443/mcp` is `mcp:url:https://mcp.example.com/mcp`.
- **Same URL as an `mcp.json` entry.** `mcp:url:` is that entry: its `oauth` fields, its variable and its grant. If several entries share the URL, the first by name wins, and `config show` lists the others.
- **URL only.** There are no `oauth` fields: the client is mcpcat's metadata document, else dynamic registration, with the default callback. There is no variable, so `get` never reads the environment and `get --json` reports `"env": []`. The grant goes in the OS keyring under `slicc-jerk` / `mcp:url:<canonical url>/url`. `import-pi` looks the URL up in `mcp-auth.json` and exits 5 when several pi servers use it. `jerk list` doesn't show URL-only ids.
- Server names in `mcp.json` that start with `url:` are skipped.

</details>

## Moving a tab's session

`jerk tab` copies one tab's signed-in state (its cookies, `localStorage` and `sessionStorage`) into another tab, in the same browser or a different one. It's like `playwright teleport` from [slicc-cdp](https://github.com/ai-ecoverse/slicc-cdp), but you pick the tab and the state goes through a file or a pipe. It works with Chromium browsers.

```sh
jerk tab list --cdp 9222                                        # tab ids, URLs and titles
jerk tab export --url mail.example.com --cdp 9222 > tab.bundle
jerk tab import --cdp 9333 < tab.bundle                         # a new tab in another browser
jerk tab export --tab 3F2A --cdp 9222 | jerk tab import --tab 9C1B --cdp 9222
```

- **Picking the tab:** `--tab` takes a target id or a unique prefix of one. `--url` matches part of the URL, or the whole URL when the pattern contains `*`. If more than one tab matches, jerk lists them and exits 2.
- **The browser:** `--cdp` takes a port (`9222` is `http://127.0.0.1:9222`), an `http(s)` DevTools URL or a `ws(s)` URL. Without it, jerk uses `SLICC_CDP_URL`, then `slicc browsers` ([slicc-browsers](https://github.com/ai-ecoverse/slicc-browsers)).
- **`export`** writes one base64 line: the cookies Chrome sends to the page and its frames (HttpOnly included), plus the top frame's storage. Base64 isn't encryption, so treat the bundle like a password. jerk won't print it to a terminal.
- **`import`** opens a new tab, or uses the one `--tab` or `--url` picks, sets the cookies (host-only and `__Host-` cookies stay host-only), opens the exported origin, writes the storage there once it has checked that the page really is on that origin, then opens the exported URL and prints the tab id. jerk registers no script that would stay in the tab. If the origin redirects to another one, such as a sign-in page, the storage can't be written there and jerk exits 1; if only the exported URL redirects away, the storage is already written and jerk says where the tab ended up.
- **Not carried:** IndexedDB, Cache Storage, service workers, other sites' cookies (an identity provider's, say), and anything tied to the device, such as client certificates or device-bound session credentials. Some sites also tie a session to its IP address or user agent.
- jerk never opens, exports or reads cookies of a seven tab, and on import it leaves seven's cookies out of a bundle that has any.

## fnox

jerk is a `command` lease backend for [fnox](https://fnox.jdx.dev). `jerk config fnox` prints the stanzas for `~/.config/fnox/config.toml`, and `--proxy` adds `[[proxy.rules]]` for the provider's domains:

```toml
[leases.github]
type = "command"
create_command = "jerk github lease"
revoke_command = "jerk github drop --lease"
duration = "1h"
```

- **`lease`** prints fnox's lease JSON: every `env` name mapped to the token, an `expires_at` that is the earlier of the token's own expiry and now + `FNOX_LEASE_DURATION`, and a `lease_id`. It refuses a terminal.
- **`drop --lease`** reads `FNOX_LEASE_ID` and revokes upstream only a token jerk minted for that lease. A token jerk only read is left to expire.
- **`sync --fnox <SECRET> --provider <p>`** pipes the token into `fnox set` on stdin, which is how jerk tokens reach `fnox proxy`. It only writes to fnox providers that encrypt or use an OS store (age, keychain, 1password, bitwarden, fido2, yubikey and the cloud KMSes), never `plain`. Run it again when the token is refreshed.
- **On Windows,** fnox runs lease commands with `sh -c`, so it needs an `sh` on `PATH` (Git for Windows, MSYS2 or similar).

## In SLICC

The npm package also carries `bin/jerk.wasm`, a `wasm32-wasip1` build that seven runs as a [slicc-kernel](https://github.com/ai-ecoverse/slicc-kernel) command (`slicc.abi` `wasi`). That jerk never holds a token: tokens live in the [vault](https://github.com/ai-ecoverse/slicc-vault)'s sealed store behind `/dev/sealed`, and kernel programs only ever see placeholders.

- **`jerk list`** shows seven's own `[oauth]` providers and, when a [slicc CLI](https://github.com/ai-ecoverse/slicc-cli) is linked, the providers of the machine behind it.
- **`get`** prints the placeholder, and still only the placeholder with `--reveal`. **`drop`** removes the sealed record.
- **`create`** asks the linked machine's jerk to sign in, or [bios](https://github.com/ai-ecoverse/slicc-bios) to run an `[oauth]` sign-in, and the result is sealed.
- **`tab`** drives the browser seven runs in, through the kernel's CDP endpoint in `SLICC_CDP_URL` (from the slicc extension or the slicc CLI); `--cdp 9222` names the same endpoint. The kernel build has no TLS, so `wss://` and `https://` don't work. A bundle exported in seven is plain text, so keep it out of shared folders.
- **`<provider> list`, `lease`, `sync` and `intercept`** exit 5.

On the machine side, the slicc CLI runs jerk with `--via-link`. `create --via-link` prints progress and a final status as newline-delimited JSON and never prompts; it refuses `--force`, minted tokens and sign-ins that need a terminal. `lease --via-link` prints the lease, then a status. `drop --lease --via-link` prints a status; a CLI sign-out never runs over the link.

## Security

- **Printing tokens:** jerk prints a token only to a terminal or with `--reveal`, and never logs one. Errors never repeat tokens, codes, verifiers, `state`, config values or response bodies.
- **Tokens at rest:** CLI providers' tokens stay in the CLI's own store, except a token a `create` with `keep = true` prints once or a key pasted into an `ask` create. jerk's own OAuth grants live in the OS keyring, or a 0600 file only when the config says so.
- **URLs:** OAuth and discovery URLs must be https, except http on loopback, with no userinfo. A captured redirect must match `redirect_uri` exactly.
- **Child processes:** provider CLIs run from argument lists, never a shell, and without the provider's token variables.

## `@ai-ecoverse/jerk-oauth`

jerk's OAuth core, compiled to WebAssembly for the browser (ESM, wasm-bindgen `web` target). [bios](https://github.com/ai-ecoverse/slicc-bios) uses it for sign-ins in seven, and the [vault](https://github.com/ai-ecoverse/slicc-vault) for refreshes. It does no I/O: the caller does every fetch, opens every popup, and supplies the clock and the random bytes.

<details>
<summary>API</summary>

```js
import init, { parseConfig, beginSignIn, completeSignIn, parseTokenResponse, parseRefreshResponse,
  refreshRequest, refreshTokenRequest, refreshTokenRequestBytes, scopesSatisfied, shouldBackoff } from '@ai-ecoverse/jerk-oauth';
```

| function | returns |
| --- | --- |
| `parseConfig(tomlOrJson)` | `{ ok, config?, errors? }` for an `[oauth]` block |
| `beginSignIn(config, scope, nowSecs, random32, random16)` | `{ authorizeUrl, state, expectedNonce?, pending }`; keep `pending` in memory |
| `completeSignIn(config, pending, capturedUrl)` | `{ kind: "exchange", url, body, contentType }` to POST, or `{ kind: "token", … }` for implicit flows |
| `parseTokenResponse(config, body, nowSecs)` | `{ accessToken, refreshToken?, expiresAt?, scope? }`, with the token read from `exchange.token_field` when set |
| `refreshTokenRequest(paramsJson)` | `{ url, body }` from `{ token_url, client_id, refresh_token, scope?, resource?, client_secret? }`, no sign-in config needed |
| `refreshTokenRequestBytes(bytes)` | the same, and it zeroes the caller's buffer |
| `parseRefreshResponse(body, nowSecs)` | like `parseTokenResponse`, no config needed |
| `refreshRequest(config, refreshToken)` | `{ url, body }` from a full config |
| `scopesSatisfied(granted, requested)` | boolean |
| `shouldBackoff(lastFailureSecs, nowSecs)` | boolean, a 5-minute window after a failed refresh |

Errors are thrown as `{ kind: "error", code, message }`. Provider errors (`code: "idp_error"`) also carry `error` and `description`, cut to 128 printable ASCII characters; show `code` and `error` rather than the provider's free text. `nowSecs` must be whole, non-negative seconds (`invalid_clock` otherwise). The random bytes must come from `crypto.getRandomValues`; jerk only checks they aren't constant.

</details>

## Related projects

- [slicc](https://github.com/ai-ecoverse/slicc): the browser-native agent jerk was built for.
- [slicc-cli](https://github.com/ai-ecoverse/slicc-cli): links a running seven to this machine, installs jerk, and runs it over that link.
- [slicc-kernel](https://github.com/ai-ecoverse/slicc-kernel): seven's wasm process kernel, which runs `jerk.wasm`.
- [slicc-vault](https://github.com/ai-ecoverse/slicc-vault): the sealed store for tokens in seven, refreshed with `jerk-oauth`.
- [slicc-bios](https://github.com/ai-ecoverse/slicc-bios): boots seven and runs its `[oauth]` sign-ins with `jerk-oauth`.
- [slicc-cdp](https://github.com/ai-ecoverse/slicc-cdp) and [slicc-browsers](https://github.com/ai-ecoverse/slicc-browsers): CDP, `playwright teleport`, and the browser discovery jerk uses.
- [mcpcat](https://github.com/ai-ecoverse/mcpcat): a curl for MCP, which signs in through jerk.
- [fnox](https://fnox.jdx.dev): secrets and leases, with jerk as a lease backend.

## Development

- `npm run lint`: slicc lint, shellcheck, rustfmt and clippy.
- `npm test`: builds jerk and runs `test/integration/`.
- `npm run test:unit`: the gitignored unit tests in `test/unit/`, with coverage through `cargo-llvm-cov`.
- `npm run build:wasm`: the kernel build, `bin/jerk.wasm`.
- `npm run build:oauth-wasm`: `@ai-ecoverse/jerk-oauth`, into `dist/jerk-oauth/`, with the prebuilt `wasm-bindgen` of the version in `Cargo.lock`.
- `npm run lint:wasm`: two `jerk-oauth` builds that must be byte-identical.

CI is `.github/workflows/ci.yml`, one caller of [slicc-shared-web's pipeline](https://github.com/ai-ecoverse/slicc-shared-web#pipeline), on Linux: lint, the integration tests in shards, and a Claude and a Codex review on pull requests. Every merge to `main` goes through semantic-release, which tags a release and opens it as a draft. When there's a new tag, the jobs after `released` build the six native binaries and `jerk.wasm`, sign and notarize the macOS ones, write `SHA256SUMS`, attest them, upload them with `install.sh`, publish the eight npm packages with provenance and then publish the release. If one of those jobs fails, re-run it from the Actions run.

## License

Apache-2.0
