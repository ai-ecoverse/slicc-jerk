import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';
import { stubCdp } from './cdp-stub.mjs';
import { sandbox } from './helpers.mjs';

function idp() {
  const posts = [];
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (chunk) => {
      body += chunk;
    });
    req.on('end', () => {
      posts.push({ url: req.url, body });
      res.writeHead(200, { 'content-type': 'application/json' });
      if (req.url === '/device') {
        res.end(
          JSON.stringify({
            device_code: 'dc',
            user_code: 'UC',
            verification_uri: 'https://idp.example.invalid/verify',
            expires_in: 900,
            interval: 0,
          })
        );
      } else {
        res.end(
          JSON.stringify({
            access_token: 'at-FAKE-1234567890',
            token_type: 'Bearer',
            refresh_token: 'rt-FAKE',
            expires_in: 3600,
          })
        );
      }
    });
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      resolve({
        port: server.address().port,
        posts,
        close: () => new Promise((done) => server.close(done)),
      });
    });
  });
}

test('create says already signed in when the grant has no recorded scope', async () => {
  const sb = sandbox();
  const server = await idp();
  try {
    sb.writeUser(`[providers.noscope]
env = ["NOSCOPE_TOKEN"]
store = "file"
params = { scope = "openid profile" }
[providers.noscope.oauth]
mode = "device"
flow = "pkce"
authorize_url = "http://127.0.0.1:${server.port}/device"
token_url = "http://127.0.0.1:${server.port}/token"
client_id = "cid"
redirect_uri = "http://127.0.0.1:1/cb"
scope = "openid profile"
capture = { at = "request", from = "query", param = "code" }
`);
    const first = await sb.jerkAsync(['noscope', 'create', '--force']);
    assert.equal(first.status, 0, first.stderr);
    assert.equal(first.stdout, 'noscope: signed in\n');

    const again = await sb.jerkAsync(['noscope', 'create']);
    assert.equal(again.status, 0, again.stderr);
    assert.equal(again.stdout, 'noscope: already signed in; use --force to sign in again\n');
    assert.equal(server.posts.filter((p) => p.url === '/device').length, 1);

    const wider = await sb.jerkAsync(['noscope', 'create', '--scope', 'admin']);
    assert.equal(wider.status, 0, wider.stderr);
    assert.equal(wider.stdout, 'noscope: signed in\n');
    assert.equal(server.posts.filter((p) => p.url === '/device').length, 2);

    const param = await sb.jerkAsync(['noscope', 'create', '-p', 'scope=admin']);
    assert.equal(param.status, 0, param.stderr);
    assert.equal(param.stdout, 'noscope: signed in\n');
    assert.equal(server.posts.filter((p) => p.url === '/device').length, 3);
  } finally {
    await server.close();
  }
});

test('intercept create renders a built-in placeholder in capture.url', async () => {
  const sb = sandbox();
  const token = 'cap-FAKETOKENVALUE1234';
  sb.writeUser(`[providers.capurl]
env = ["CAP_TOKEN"]
store = "file"
[providers.capurl.oauth]
mode = "intercept"
flow = "implicit"
authorize_url = "https://app.example/login"
client_id = "app-id"
client_secret = "sekret"
redirect_uri = "https://app.example/"
timeout = "5s"
capture = { at = "network", from = "body", stage = "request", url = "https://{client_id}.example/api/*?s={client_secret}", param = "token" }
`);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 0, check.stderr);
  assert.match(check.stdout, /ok: 20 providers/);

  const cdp = await stubCdp({
    paused: [
      {
        resourceType: 'XHR',
        request: {
          url: 'https://app-id.example/api/session?s=sekret',
          method: 'POST',
          headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
          postData: `token=${token}`,
        },
      },
    ],
  });
  try {
    const created = await sb.jerkAsync(['capurl', 'create', '--force', '--cdp', cdp.wsUrl], {
      SLICC_JERK_STORE: 'file',
    });
    assert.equal(created.status, 0, `${created.stderr}\n--- cdp ---\n${cdp.log()}`);
    assert.match(created.stdout, /capurl: signed in/);
    assert.match(cdp.log(), /"urlPattern":"https:\/\/app-id\.example\/api\/\*\?s=sekret"/);
    assert.doesNotMatch(cdp.log(), /\{client_id\}|\{client_secret\}/);
    const got = sb.jerk(['capurl', 'get', '--reveal'], { SLICC_JERK_STORE: 'file' });
    assert.equal(got.stdout.trim(), token);
  } finally {
    await cdp.close();
  }
});

test('config check rejects capture.stage unless the capture is a network body', () => {
  const sb = sandbox();
  sb.writeUser(`[providers.hdrstage]
env = ["HDR_TOKEN"]
[providers.hdrstage.oauth]
mode = "intercept"
flow = "implicit"
authorize_url = "https://app.example/login"
client_id = "app-id"
redirect_uri = "https://app.example/"
capture = { at = "network", from = "header", header = "authorization", stage = "response", url = "https://api.example/*", param = "authorization" }
`);
  const rejected = sb.jerk(['config', 'check']);
  assert.equal(rejected.status, 2);
  assert.match(
    rejected.stderr,
    /hdrstage: oauth: capture\.stage: only applies to a network body capture/
  );

  sb.writeUser(`[providers.bodystage]
env = ["BODY_TOKEN"]
[providers.bodystage.oauth]
mode = "intercept"
flow = "implicit"
authorize_url = "https://app.example/login"
client_id = "app-id"
redirect_uri = "https://app.example/"
capture = { at = "network", from = "body", stage = "request", url = "https://api.example/*", param = "token" }
`);
  const accepted = sb.jerk(['config', 'check']);
  assert.equal(accepted.status, 0, accepted.stderr);
  assert.match(accepted.stdout, /ok: 20 providers/);
});

test('config check rejects capture.url builtins the flow cannot fill', () => {
  const sb = sandbox();
  sb.writeUser(`[providers.missingbuilt]
env = ["MISSING_TOKEN"]
[providers.missingbuilt.oauth]
mode = "intercept"
flow = "implicit"
authorize_url = "https://app.example/login"
client_id = "app-id"
redirect_uri = "https://app.example/"
capture = { at = "network", from = "body", url = "https://api.example/{code_challenge}?s={client_secret}", param = "token" }
`);
  const rejected = sb.jerk(['config', 'check']);
  assert.equal(rejected.status, 2);
  assert.match(rejected.stderr, /placeholder \{code_challenge\} is only available for pkce/);
  assert.match(rejected.stderr, /placeholder \{client_secret\} needs client_secret/);

  sb.writeUser(`[providers.pkceurl]
env = ["PKCE_TOKEN"]
[providers.pkceurl.oauth]
mode = "intercept"
flow = "pkce"
authorize_url = "https://app.example/login"
token_url = "https://app.example/token"
client_id = "app-id"
client_secret = "sekret"
redirect_uri = "https://app.example/cb"
capture = { at = "network", from = "body", url = "https://{client_id}.example/api/*?c={code_challenge}&s={client_secret}", param = "token" }
`);
  const accepted = sb.jerk(['config', 'check']);
  assert.equal(accepted.status, 0, accepted.stderr);
  assert.match(accepted.stdout, /ok: 20 providers/);
});
