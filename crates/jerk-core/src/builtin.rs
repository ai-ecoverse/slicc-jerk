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

[providers.claude]
name = "Anthropic (Claude Code)"
env = ["ANTHROPIC_API_KEY"]
domains = ["api.anthropic.com"]
requires = "claude"
max_age = "365d"
get = { keep = true }
list = { run = ["claude", "auth", "status", "--json"], parse = "json:", min_version = "2.1.41" }
create = { run = ["claude", "setup-token"], parse = "scan:sk-ant-oat", interactive = true, echo = true, keep = true }
drop = { keep = true }

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

[providers.gcloud]
name = "Google Cloud (gcloud)"
env = ["CLOUDSDK_AUTH_ACCESS_TOKEN"]
domains = ["www.googleapis.com", "oauth2.googleapis.com", "cloudresourcemanager.googleapis.com"]
max_age = "55m"
list = { run = ["gcloud", "auth", "list", "--format=json"], parse = "json:", requires = "gcloud" }
user = { run = ["gcloud", "config", "get-value", "account"], parse = "text", requires = "gcloud" }

[[providers.gcloud.get]]
requires = "gcloud"
run = ["gcloud", "auth", "print-access-token"]
parse = "text"

[[providers.gcloud.get]]
oauth = true

[[providers.gcloud.create]]
requires = "gcloud"
run = ["gcloud", "auth", "login"]
interactive = true

[[providers.gcloud.create]]
oauth = true

[[providers.gcloud.drop]]
requires = "gcloud"
run = ["gcloud", "auth", "revoke", "{id}"]
interactive = true

[[providers.gcloud.drop]]
oauth = true

[providers.gcloud.oauth]
mode = "loopback"
flow = "code"
authorize_url = "https://accounts.google.com/o/oauth2/auth?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&state={state}&access_type=offline&prompt=consent"
token_url = "https://oauth2.googleapis.com/token"
client_id = "32555940559.apps.googleusercontent.com"
client_secret = "ZmssLNjJy2998hD4CTg2ejr2"
redirect_uri = "http://127.0.0.1:8085/"
scope = "openid https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/cloud-platform"
capture = { at = "request", from = "query", param = "code" }

[providers.adobe-aio]
name = "Adobe IMS (aio CLI)"
env = ["ADOBE_IMS_TOKEN", "AEM_EDGE_FUNCTIONS_TOKEN"]
domains = ["ims-na1.adobelogin.com", "firefall.adobe.io", "admin.da.live"]
params = { ctx = "cli", client_id = "", client_secret = "", scope = "openid,AdobeID,aem.cdn,additional_info.projectedProductContext" }

[[providers.adobe-aio.get]]
requires = "aio"
run = ["aio", "login", "--bare", "--no-open", "--ctx", "{ctx}"]
parse = "text"
expiry = "jwt"
timeout = "10s"
unless = ["client_id"]

[[providers.adobe-aio.get]]
oauth = true
needs = ["client_id", "client_secret"]

[[providers.adobe-aio.create]]
requires = "aio"
run = ["aio", "login", "--force", "--ctx", "{ctx}"]
interactive = true
link = true
unless = ["client_id"]

[[providers.adobe-aio.create]]
oauth = true
needs = ["client_id", "client_secret"]

[[providers.adobe-aio.drop]]
requires = "aio"
run = ["aio", "logout", "--ctx", "{ctx}"]
interactive = true
unless = ["client_id"]

[[providers.adobe-aio.drop]]
oauth = true
needs = ["client_id"]

[providers.adobe-aio.oauth]
mode = "client_credentials"
token_url = "https://ims-na1.adobelogin.com/ims/token/v3"
client_id = "{client_id}"
client_secret = "{client_secret}"
scope = "{scope}"

[providers.firefly]
name = "Adobe Firefly Services"
env = ["FIREFLY_ACCESS_TOKEN", "ADOBE_IMS_TOKEN"]
domains = ["firefly-api.adobe.io", "ims-na1.adobelogin.com"]
params = { client_id = "", client_secret = "", org_id = "", scope = "openid,AdobeID,session,additional_info,firefly_api,ff_apis,read_organizations,additional_info.projectedProductContext,creative_sdk,ee.express_api,indesign_services" }
[providers.firefly.oauth]
mode = "client_credentials"
token_url = "https://ims-na1.adobelogin.com/ims/token/v3"
client_id = "{client_id}"
client_secret = "{client_secret}"
scope = "{scope}"

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

[providers.microsoft-graph]
name = "Microsoft Graph (az / Entra)"
env = ["MICROSOFT_GRAPH_TOKEN", "EXCEL_GRAPH_TOKEN"]
domains = [
  "graph.microsoft.com",
  "outlook.office.com",
  "outlook.office365.com",
  "outlook.cloud.microsoft",
]
params = { tenant = "common" }

[[providers.microsoft-graph.get]]
requires = "az"
run = ["az", "account", "get-access-token", "--resource-type", "ms-graph", "-o", "json"]
parse = "json:/accessToken"
expiry = "jwt"

[[providers.microsoft-graph.get]]
oauth = true

[[providers.microsoft-graph.list]]
requires = "az"
run = ["az", "account", "list", "-o", "json"]
parse = "json:"

[[providers.microsoft-graph.user]]
requires = "az"
run = ["az", "account", "show", "--query", "user.name", "-o", "tsv"]
parse = "text"

[[providers.microsoft-graph.create]]
requires = "az"
run = ["az", "login", "--tenant", "{tenant}"]
interactive = true

[[providers.microsoft-graph.create]]
oauth = true

[[providers.microsoft-graph.drop]]
requires = "az"
run = ["az", "logout"]
interactive = true

[[providers.microsoft-graph.drop]]
oauth = true

[providers.microsoft-graph.oauth]
mode = "auto"
flow = "pkce"
authorize_url = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id={client_id}&response_type=code&redirect_uri={redirect_uri}&response_mode=query&scope={scope}&state={state}&code_challenge={code_challenge}&code_challenge_method=S256"
token_url = "https://login.microsoftonline.com/common/oauth2/v2.0/token"
client_id = "14d82eec-204b-4c2f-b7e0-23000bd7a84"
redirect_uri = "http://localhost:56123"
scope = "offline_access openid profile User.Read Mail.ReadWrite Mail.Send Calendars.ReadWrite Files.ReadWrite.All Sites.ReadWrite.All"
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

[providers.gmail]
name = "Gmail (Google OAuth)"
env = ["GWS_ACCESS_TOKEN"]
domains = ["gmail.googleapis.com", "www.googleapis.com", "mail.google.com"]
[providers.gmail.oauth]
mode = "loopback"
flow = "pkce"
authorize_url = "https://accounts.google.com/o/oauth2/auth?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&state={state}&code_challenge={code_challenge}&code_challenge_method=S256&access_type=offline&prompt=consent"
token_url = "https://oauth2.googleapis.com/token"
client_id = "${GWS_CLIENT_ID:-406964657835-aq8lmia8j95dhl1a2bvharmfk3t1hgqj.apps.googleusercontent.com}"
client_secret = "${GWS_CLIENT_SECRET:-kSmqreRr0qwBWJgbf5Y-PjSU}"
redirect_uri = "http://127.0.0.1:56121"
scope = "https://mail.google.com/"
capture = { at = "request", from = "query", param = "code" }
revoke_url = "https://oauth2.googleapis.com/revoke"
refresh_env = "GWS_REFRESH_TOKEN"

[providers.rescuetime]
name = "RescueTime"
env = ["RESCUETIME_API_KEY"]
domains = ["www.rescuetime.com"]
[providers.rescuetime.oauth]
mode = "intercept"
flow = "implicit"
authorize_url = "https://www.rescuetime.com/anapi/manage"
client_id = "rescuetime"
redirect_uri = "https://www.rescuetime.com/rtx/settings/api/key_setup_summary"
capture = { at = "network", from = "body", url = "https://www.rescuetime.com/anapi/setup_submit", param = "candidate" }
timeout = "10m"

[providers.garmin]
name = "Garmin Connect"
env = ["GARMIN_TOKEN"]
domains = ["connectapi.garmin.com", "diauth.garmin.com", "connect.garmin.com", "sso.garmin.com"]
[providers.garmin.oauth]
mode = "intercept"
flow = "exchange"
authorize_url = "https://sso.garmin.com/sso/signin?id=gauth-widget&clientId=GarminConnect&locale=en&service=https%3A%2F%2Fconnect.garmin.com%2Fmodern&webhost=https%3A%2F%2Fconnect.garmin.com%2Fmodern&redirectAfterAccountLoginUrl=https%3A%2F%2Fconnect.garmin.com%2Fmodern&redirectAfterAccountCreationUrl=https%3A%2F%2Fconnect.garmin.com%2Fmodern"
token_url = "https://diauth.garmin.com/di-oauth2-service/oauth/token"
client_id = "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2"
redirect_uri = "https://connect.garmin.com/modern"
capture = { at = "request", from = "query", param = "ticket" }
on_capture = "close"
timeout = "5m"
[providers.garmin.oauth.exchange]
grant_type = "https://connectapi.garmin.com/di-oauth2-service/oauth/grant/service_ticket"
body = { service_ticket = "{value}", service_url = "https://connect.garmin.com/modern" }

[providers.slack]
name = "Slack"
env = ["SLACK_TOKEN", "SLACK_D_COOKIE"]
domains = ["app.slack.com", "edgeapi.slack.com", "files.slack.com"]
params = { workspace = "" }
[providers.slack.oauth]
mode = "intercept"
flow = "implicit"
authorize_url = "https://app.slack.com/client/{workspace}"
client_id = "slack"
redirect_uri = "https://app.slack.com/"
capture = { at = "network", from = "body", stage = "request", url = "https://*.slack.com/api/*", param = "token", fields = { SLACK_D_COOKIE = "cookie:d" } }
timeout = "2m"
"#;

#[cfg(target_os = "wasi")]
pub const BUILTIN: &str = "";
