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
create = { run = ["npm", "token", "create", "--name", "{name}", "--expires", "{expires}", "--json"], parse = "json:/token", id = "json:/key", interactive = true }
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
"#;

#[cfg(target_os = "wasi")]
pub const BUILTIN: &str = "";
