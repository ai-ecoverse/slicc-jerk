#[cfg(not(target_os = "wasi"))]
pub const BUILTIN: &str = r#"
[providers.github]
name = "GitHub (gh)"
env = ["GH_TOKEN", "GITHUB_TOKEN"]
domains = ["api.github.com", "github.com", "uploads.github.com", "raw.githubusercontent.com"]
requires = "gh"
params = { host = "github.com" }
get = { run = ["gh", "auth", "token", "--hostname", "{host}"], parse = "text" }
list = { run = ["gh", "auth", "status", "--hostname", "{host}", "--json", "hosts"], parse = "json:/hosts" }
scopes = { run = ["gh", "auth", "status", "--hostname", "{host}", "--active", "--json", "hosts"], parse = "json:/hosts/{host}/0/scopes" }
user = { run = ["gh", "auth", "status", "--hostname", "{host}", "--active", "--json", "hosts"], parse = "json:/hosts/{host}/0/login" }
create = { run = ["gh", "auth", "login", "--web", "--hostname", "{host}", "--scopes", "{scope}"], interactive = true }
drop = { run = ["gh", "auth", "logout", "--hostname", "{host}", "--user", "{id}"], interactive = true }

[providers.npm]
name = "npm registry"
env = ["NPM_TOKEN"]
domains = ["registry.npmjs.org"]
requires = "npm"
params = { registry = "//registry.npmjs.org/", expires = "1", name = "jerk" }
lease_via = "get"
get = { read = { ini = "${NPM_CONFIG_USERCONFIG:-~/.npmrc}", key = "{registry}:_authToken" } }
list = { run = ["npm", "token", "list", "--json"], parse = "json:" }
create = { run = ["npm", "token", "create", "--name", "{name}", "--expires", "{expires}", "--json"], parse = "json:/token", interactive = true, min_version = "11.6.3", lookup = { run = ["npm", "token", "list", "--json"], prefix = "/token", id = "/key" } }
drop = { run = ["npm", "token", "revoke", "{id}"] }

[providers.cloudflare]
name = "Cloudflare (wrangler)"
env = ["CLOUDFLARE_API_TOKEN"]
domains = ["api.cloudflare.com"]
requires = "wrangler"
max_age = "50m"
get = { run = ["wrangler", "auth", "token", "--json"], parse = "json:/token" }
list = { run = ["wrangler", "whoami", "--json"], parse = "json:" }
create = { run = ["wrangler", "login", "--scopes", "{scope}"], interactive = true }
drop = { run = ["wrangler", "logout"], interactive = true }

[providers.adobe-aio]
name = "Adobe IMS (aio CLI)"
env = ["ADOBE_IMS_TOKEN"]
domains = ["ims-na1.adobelogin.com", "firefall.adobe.io", "admin.da.live"]
requires = "aio"
params = { ctx = "cli" }
get = { run = ["aio", "login", "--bare", "--no-open", "--ctx", "{ctx}"], parse = "text", expiry = "jwt", timeout = "10s" }
create = { run = ["aio", "login", "--force", "--ctx", "{ctx}"], interactive = true, link = true }
drop = { run = ["aio", "logout", "--ctx", "{ctx}"], interactive = true }

[providers.aem]
name = "AEM Admin (api.aem.live)"
env = ["AEM_TOKEN"]
domains = ["api.aem.live"]
params = { org = "", site = "", login_host = "admin.hlx.page" }
[providers.aem.oauth]
mode = "loopback"
flow = "implicit"
authorize_url = "https://{login_host}/login/{org}/{site}/main?client_id={client_id}&redirect_uri={redirect_uri}&state={state}&selectAccount=true"
client_id = "aem-cli"
redirect_uri = "http://localhost:56122/.aem/cli/login/ack"
capture = { at = "callback", from = "body", param = "authToken" }
timeout = "5m"

[providers.xai]
name = "xAI Grok"
env = ["XAI_API_KEY"]
domains = ["api.x.ai"]
[providers.xai.oauth]
mode = "auto"
flow = "pkce"
authorize_url = "https://auth.x.ai/oauth2/auth?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}&nonce={nonce}&plan=generic&referrer=slicc"
token_url = "https://auth.x.ai/oauth2/token"
client_id = "b1a00492-073a-47ea-816f-4c329264a828"
redirect_uri = "http://127.0.0.1:56121/callback"
scope = "openid profile email offline_access grok-cli:access api:access"
capture = { at = "request", from = "query", param = "code" }
rewrite = [{ match = "auth.x.ai", append = { plan = "generic" } }]
"#;

#[cfg(target_os = "wasi")]
pub const BUILTIN: &str = "";
