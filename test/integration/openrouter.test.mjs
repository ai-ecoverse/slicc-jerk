import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import { setTimeout as sleep } from 'node:timers/promises';
import { listen } from './cdp-stub.mjs';
import { sandbox } from './helpers.mjs';

const fakeKey = 'sk-or-v1-FAKEOPENROUTERKEY1234567890';

test('openrouter is a built-in loopback pkce provider that keeps the returned key', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'openrouter']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /mode = "loopback"/);
  assert.match(shown.stdout, /flow = "pkce"/);
  assert.match(shown.stdout, /OPENROUTER_API_KEY/);
  assert.match(shown.stdout, /domains = \["openrouter\.ai"\]/);
  assert.match(
    shown.stdout,
    /https:\/\/openrouter\.ai\/auth\?callback_url=\{redirect_uri\}&code_challenge=\{code_challenge\}&code_challenge_method=S256/
  );
  assert.match(shown.stdout, /token_url = "https:\/\/openrouter\.ai\/api\/v1\/auth\/keys"/);
  assert.match(shown.stdout, /format = "json"/);
  assert.match(shown.stdout, /token_field = "key"/);
  assert.match(shown.stdout, /code_verifier = "\{code_verifier\}"/);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 0, check.stderr);
});

test('openrouter get uses OPENROUTER_API_KEY and otherwise asks for create', () => {
  const sb = sandbox();
  const env = sb.jerk(['openrouter', 'get', '--reveal'], { OPENROUTER_API_KEY: fakeKey });
  assert.equal(env.status, 0, env.stderr);
  assert.equal(env.stdout, `${fakeKey}\n`);
  const missing = sb.jerk(['openrouter', 'get']);
  assert.equal(missing.status, 3);
  assert.match(missing.stderr, /jerk openrouter create/);
});

test('config check rejects a bad exchange token_field and format', () => {
  const sb = sandbox();
  sb.writeUser(`[providers.badfield]
env = ["BAD_TOKEN"]
[providers.badfield.oauth]
mode = "loopback"
flow = "pkce"
authorize_url = "https://app.example/auth?c={code_challenge}"
token_url = "https://app.example/keys"
client_id = "app"
redirect_uri = "http://127.0.0.1:1/cb"
capture = { at = "request", from = "query", param = "code" }
[providers.badfield.oauth.exchange]
format = "json"
token_field = "a/b"
body = { code = "{value}" }
`);
  const bad = sb.jerk(['config', 'check']);
  assert.equal(bad.status, 2);
  assert.match(bad.stderr, /exchange\.token_field/);

  sb.writeUser(`[providers.badformat]
env = ["BAD_TOKEN"]
[providers.badformat.oauth]
mode = "loopback"
flow = "pkce"
authorize_url = "https://app.example/auth?c={code_challenge}"
token_url = "https://app.example/keys"
client_id = "app"
redirect_uri = "http://127.0.0.1:1/cb"
capture = { at = "request", from = "query", param = "code" }
[providers.badformat.oauth.exchange]
format = "xml"
body = { code = "{value}" }
`);
  const format = sb.jerk(['config', 'check']);
  assert.equal(format.status, 2);
});

test('openrouter create sends the S256 challenge, exchanges JSON and keeps the key with no expiry', {
  skip: process.platform === 'win32',
}, async () => {
  const sb = sandbox();
  const posts = [];
  const idp = await listen((req, res) => {
    let body = '';
    req.on('data', (c) => {
      body += c;
    });
    req.on('end', () => {
      posts.push({ url: req.url, type: req.headers['content-type'], body });
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ key: fakeKey, user_id: 'user_FAKE' }));
    });
  });
  const free = await listen(() => {});
  const port = free.port;
  await free.close();
  sb.script(process.platform === 'darwin' ? 'open' : 'xdg-open', '');
  sb.writeUser(`[providers.openrouter]
name = "OpenRouter"
env = ["OPENROUTER_API_KEY"]
domains = ["openrouter.ai"]
store = "file"
[providers.openrouter.oauth]
mode = "loopback"
flow = "pkce"
authorize_url = "https://openrouter.ai/auth?callback_url={redirect_uri}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}&key_label={client_id}"
token_url = "http://127.0.0.1:${idp.port}/api/v1/auth/keys"
client_id = "slicc-jerk"
redirect_uri = "http://localhost:${port}/callback"
capture = { at = "request", from = "query", param = "code" }
timeout = "20s"
[providers.openrouter.oauth.exchange]
format = "json"
token_field = "key"
body = { code = "{value}", code_verifier = "{code_verifier}", code_challenge_method = "S256" }
`);
  try {
    const running = sb.jerkAsync(['openrouter', 'create', '--force']);
    let opened;
    for (let i = 0; i < 200 && !opened; i++) {
      opened = sb.calls().find((c) => c.name === 'open' || c.name === 'xdg-open');
      if (!opened) await sleep(50);
    }
    assert.ok(opened, 'the browser was not opened');
    const auth = new URL(opened.args.at(-1));
    assert.equal(auth.origin + auth.pathname, 'https://openrouter.ai/auth');
    assert.equal(auth.searchParams.get('callback_url'), `http://localhost:${port}/callback`);
    assert.equal(auth.searchParams.get('code_challenge_method'), 'S256');
    const challenge = auth.searchParams.get('code_challenge');
    assert.match(challenge, /^[A-Za-z0-9_-]{43}$/);
    const state = auth.searchParams.get('state');
    const cb = await fetch(
      `http://127.0.0.1:${port}/callback?code=FAKECODE123&state=${encodeURIComponent(state)}`
    );
    assert.equal(cb.status, 200);
    await cb.text();
    const created = await running;
    assert.equal(created.status, 0, created.stderr);
    assert.equal(created.stdout, 'openrouter: signed in\n');

    assert.equal(posts.length, 1);
    assert.equal(posts[0].url, '/api/v1/auth/keys');
    assert.match(posts[0].type, /^application\/json/);
    const sent = JSON.parse(posts[0].body);
    assert.deepEqual(Object.keys(sent).sort(), ['code', 'code_challenge_method', 'code_verifier']);
    assert.equal(sent.code, 'FAKECODE123');
    assert.equal(sent.code_challenge_method, 'S256');
    assert.equal(createHash('sha256').update(sent.code_verifier).digest('base64url'), challenge);

    const got = sb.jerk(['openrouter', 'get', '--reveal', '--json']);
    assert.equal(got.status, 0, got.stderr);
    const info = JSON.parse(got.stdout);
    assert.equal(info.token, fakeKey);
    assert.equal(info.expires_at, null);
    const again = sb.jerk(['openrouter', 'get', '--reveal']);
    assert.equal(again.stdout, `${fakeKey}\n`);
    assert.equal(posts.length, 1);

    const dropped = sb.jerk(['openrouter', 'drop', '--yes']);
    assert.equal(dropped.status, 0, dropped.stderr);
    assert.equal(dropped.stdout, 'openrouter: dropped\n');
    const gone = sb.jerk(['openrouter', 'get']);
    assert.equal(gone.status, 3);
    assert.equal(posts.length, 1);
  } finally {
    await idp.close();
  }
});
