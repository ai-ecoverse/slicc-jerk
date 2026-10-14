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
name = "Cloudflare"
env = ["CLOUDFLARE_API_TOKEN"]
domains = ["api.cloudflare.com"]
max_age = "50m"
params = { name = "jerk", policies = "" }

[[providers.cloudflare.get]]
requires = "cf"
run = ["cf", "accounts", "tokens", "create", "--quiet", "--name", "{name}", "--policies", "{policies}"]
parse = "json:/value"

[[providers.cloudflare.get]]
requires = "wrangler"
run = ["wrangler", "auth", "token", "--json"]
parse = "json:/token"
min_version = "4.57.0"

[[providers.cloudflare.get]]
oauth = true

[[providers.cloudflare.list]]
requires = "cf"
run = ["cf", "auth", "whoami"]
parse = "json:"

[[providers.cloudflare.list]]
requires = "wrangler"
run = ["wrangler", "whoami", "--json"]
parse = "json:"
min_version = "4.65.0"

[[providers.cloudflare.create]]
requires = "cf"
run = ["cf", "auth", "login", "--scopes", "{scope}"]
interactive = true
min_version = "0.4.0"

[[providers.cloudflare.create]]
requires = "wrangler"
run = ["wrangler", "login", "--scopes", "{scope}"]
interactive = true
min_version = "4.57.0"

[[providers.cloudflare.create]]
oauth = true

[[providers.cloudflare.drop]]
requires = "cf"
run = ["cf", "auth", "logout"]
interactive = true

[[providers.cloudflare.drop]]
requires = "wrangler"
run = ["wrangler", "logout"]
interactive = true

[[providers.cloudflare.drop]]
oauth = true

[providers.cloudflare.oauth]
mode = "loopback"
flow = "pkce"
authorize_url = "https://dash.cloudflare.com/oauth2/auth?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&state={state}&code_challenge={code_challenge}&code_challenge_method=S256"
token_url = "https://dash.cloudflare.com/oauth2/token"
client_id = "cbca97e7-c331-4cdd-8fd8-e25a451b98bf"
redirect_uri = "http://localhost:8877/oauth/callback"
scope = "account:read user:read workers:write workers_kv:write workers_scripts:write d1:write pages:write zone:read offline_access"
capture = { at = "request", from = "query", param = "code" }

[providers.aws]
name = "AWS"
env = ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN"]
requires = "aws"
params = { profile = "", region = "" }
get = { run = ["aws", "configure", "export-credentials", "--profile", "{profile}", "--region", "{region}"], parse = "json:/AccessKeyId", fields = { AWS_SECRET_ACCESS_KEY = "json:/SecretAccessKey", AWS_SESSION_TOKEN = "json:/SessionToken" }, expiry = "json:/Expiration" }
list = { run = ["aws", "sts", "get-caller-identity", "--profile", "{profile}", "--region", "{region}"], parse = "json:" }
user = { run = ["aws", "sts", "get-caller-identity", "--profile", "{profile}", "--region", "{region}"], parse = "json:/Arn" }
create = { run = ["aws", "sso", "login", "--profile", "{profile}"], interactive = true }
drop = { run = ["aws", "sso", "logout", "--profile", "{profile}"], interactive = true }

[providers.fastly]
name = "Fastly"
env = ["FASTLY_API_TOKEN"]
domains = ["api.fastly.com"]
requires = "fastly"
params = { profile = "" }
get = { run = ["fastly", "profile", "token", "{profile}", "--quiet"], parse = "text" }
list = { run = ["fastly", "profile", "list", "--json", "--quiet"], parse = "json:" }
create = { run = ["fastly", "sso", "{profile}"], interactive = true }
drop = { run = ["fastly", "profile", "delete", "{id}", "--auto-yes"], interactive = true }

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
name = "AEM Admin (aem-ext, api.aem.live)"
env = ["AEM_TOKEN", "AEM_API_KEY"]
domains = ["api.aem.live"]
params = { org = "", site = "", login_host = "admin.hlx.page", description = "jerk", roles = "admin" }
cors_origins = [
  "https://admin.hlx.page",
  "https://admin-ci.hlx.page",
  "https://api.aem.live",
]

[[providers.aem.create]]
requires = "aem-ext"
run = ["aem-ext", "auth", "key", "create", "--org", "{org}", "--site", "{site}", "--roles", "{roles}", "--description", "{description}", "--register", "--json"]
parse = "json:/value"
id = "json:/id"
expiry = "jwt"
interactive = true

[[providers.aem.create]]
oauth = true

[[providers.aem.list]]
requires = "aem-ext"
run = ["aem-ext", "auth", "key", "list", "--org", "{org}", "--site", "{site}", "--json"]
parse = "json:/keys"

[[providers.aem.drop]]
requires = "aem-ext"
run = ["aem-ext", "auth", "key", "delete", "--org", "{org}", "--site", "{site}", "--id", "{id}", "--confirm"]

[[providers.aem.drop]]
oauth = true

[providers.aem.oauth]
mode = "loopback"
flow = "implicit"
authorize_url = "https://{login_host}/login/{org}/{site}/main?client_id={client_id}&redirect_uri={redirect_uri}&state={state}&selectAccount=true"
client_id = "aem-cli"
redirect_uri = "http://127.0.0.1:56122/.aem/cli/login/ack"
capture = { at = "callback", from = "body", param = "authToken" }
timeout = "5m"

[providers.linkedin]
name = "LinkedIn"
env = ["LINKEDIN_ACCESS_TOKEN"]
domains = ["api.linkedin.com", "www.linkedin.com"]
params = { client_id = "", client_secret = "", scope = "openid profile w_member_social" }
[providers.linkedin.oauth]
mode = "intercept"
flow = "code"
authorize_url = "https://www.linkedin.com/oauth/v2/authorization?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&state={state}"
token_url = "https://www.linkedin.com/oauth/v2/accessToken"
client_id = "{client_id}"
client_secret = "{client_secret}"
redirect_uri = "http://localhost/callback"
scope = "openid profile w_member_social"
capture = { at = "request", from = "query", param = "code" }

[providers.xai]
name = "xAI Grok"
env = ["XAI_API_KEY"]
domains = ["api.x.ai"]
[providers.xai.oauth]
mode = "auto"
flow = "pkce"
authorize_url = "https://auth.x.ai/oauth2/authorize?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}&nonce={nonce}&plan=generic&referrer=slicc"
token_url = "https://auth.x.ai/oauth2/token"
client_id = "b1a00492-073a-47ea-816f-4c329264a828"
redirect_uri = "http://127.0.0.1:56121/callback"
scope = "openid profile email offline_access grok-cli:access api:access"
capture = { at = "request", from = "query", param = "code" }
rewrite = [{ match = "auth.x.ai", append = { plan = "generic" } }]

[providers.printful]
name = "Printful"
env = ["PRINTFUL_API_TOKEN"]
domains = ["api.printful.com"]
requires = "printful"
params = { name = "jerk", days = "90", token = "" }
get = { run = ["printful", "auth", "token"], parse = "text" }
list = { run = ["printful", "auth", "status", "--json"], parse = "json:" }
user = { run = ["printful", "whoami", "--json"], parse = "json:/stores/0/name" }
create = { run = ["printful", "auth", "login", "--token", "{token}", "--name", "{name}", "--days", "{days}"], interactive = true }
drop = { run = ["printful", "auth", "logout"], interactive = true }
"#;

#[cfg(target_os = "wasi")]
pub const BUILTIN: &str = "";
