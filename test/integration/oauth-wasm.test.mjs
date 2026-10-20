import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const wasmDir = join(root, 'dist', 'jerk-oauth');
const jsPath = join(wasmDir, 'jerk_oauth_wasm.js');
const wasmPath = join(wasmDir, 'jerk_oauth_wasm_bg.wasm');
const skip = (!existsSync(jsPath) || !existsSync(wasmPath)) && 'run npm run build:oauth-wasm first';

async function loadWasm() {
  const mod = await import(pathToFileURL(jsPath).href);
  const bytes = readFileSync(wasmPath);
  mod.initSync({ module: bytes });
  return mod;
}

const pkceConfig = JSON.stringify({
  mode: 'loopback',
  flow: 'pkce',
  authorize_url:
    'https://auth.example.com/authorize?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}',
  token_url: 'https://auth.example.com/token',
  client_id: 'test-client-id',
  redirect_uri: 'http://127.0.0.1:9999/callback',
  scope: 'openid profile',
  capture: { at: 'request', from: 'query', param: 'code' },
});

const implicitRelayConfig = JSON.stringify({
  mode: 'popup',
  flow: 'implicit',
  authorize_url:
    'https://ims-na1.adobelogin.com/ims/authorize/v2?client_id={client_id}&response_type=token&scope={scope}&redirect_uri={redirect_uri}&state={state}',
  client_id: 'test-ims-client',
  redirect_uri: 'https://www.sliccy.ai/auth/callback',
  scope: 'openid AdobeID',
  capture: { at: 'callback', from: 'fragment', param: 'access_token' },
  relay: { source: 'origin', origin: 'https://test.sliccy.ai' },
  expires_param: 'expires_in',
});

const random32 = new Uint8Array(32);
for (let i = 0; i < 32; i++) random32[i] = i + 1;
const random16 = new Uint8Array(16);
for (let i = 0; i < 16; i++) random16[i] = (i + 0xa0) & 0xff;

test('parseConfig with valid JSON', { skip }, async () => {
  const wasm = await loadWasm();
  const result = wasm.parseConfig(pkceConfig);
  assert.equal(result.ok, true);
  assert.ok(result.config);
  const cfg = JSON.parse(result.config);
  assert.equal(cfg.client_id, 'test-client-id');
});

test('parseConfig with invalid JSON', { skip }, async () => {
  const wasm = await loadWasm();
  const result = wasm.parseConfig('{"mode":"loopback"}');
  assert.equal(result.ok, false);
  assert.ok(result.errors);
});

test('parseConfig never echoes an unsafe field name', { skip }, async () => {
  const wasm = await loadWasm();
  for (const key of ['a\u001b]0;x\u0007b', 'k'.repeat(420)]) {
    const result = wasm.parseConfig(JSON.stringify({ ...JSON.parse(pkceConfig), [key]: 'v' }));
    assert.equal(result.ok, false);
    const text = JSON.stringify(result.errors);
    assert.match(text, /invalid config: unknown field/);
    assert.ok(!text.includes('kkkk') && !text.includes('\\u001b'), text);
  }
  const plain = wasm.parseConfig(JSON.stringify({ ...JSON.parse(pkceConfig), extra_field: 'v' }));
  assert.match(JSON.stringify(plain.errors), /invalid config: field extra_field/);
});

test('parseConfig with valid TOML', { skip }, async () => {
  const wasm = await loadWasm();
  const toml = `
[oauth]
mode = "loopback"
flow = "pkce"
authorize_url = "https://example.com/auth?client_id={client_id}"
token_url = "https://example.com/token"
client_id = "toml-client"
redirect_uri = "http://127.0.0.1:8080/callback"
scope = "openid"

[oauth.capture]
at = "request"
from = "query"
param = "code"
`;
  const result = wasm.parseConfig(toml);
  assert.equal(result.ok, true);
  const cfg = JSON.parse(result.config);
  assert.equal(cfg.client_id, 'toml-client');
});

test('parseConfig with validation errors', { skip }, async () => {
  const wasm = await loadWasm();
  const result = wasm.parseConfig(
    JSON.stringify({
      mode: 'loopback',
      flow: 'pkce',
      authorize_url: '',
      client_id: '',
      redirect_uri: '',
      capture: { at: 'request', from: 'query', param: 'code' },
    })
  );
  assert.equal(result.ok, false);
  const errors = JSON.parse(result.errors);
  assert.ok(errors.length >= 3);
});

test('full PKCE flow', { skip }, async () => {
  const wasm = await loadWasm();
  const now = 1700000000;
  const begin = wasm.beginSignIn(pkceConfig, '', now, random32, random16);
  assert.ok(begin.authorizeUrl.startsWith('https://auth.example.com/authorize'));
  assert.ok(begin.authorizeUrl.includes('client_id=test-client-id'));
  assert.ok(begin.authorizeUrl.includes('code_challenge='));
  assert.ok(begin.state);
  assert.ok(begin.pending);
  assert.equal(begin.expectedNonce, undefined);

  const callbackUrl = `http://127.0.0.1:9999/callback?code=auth-code-123&state=${encodeURIComponent(begin.state)}`;
  const complete = wasm.completeSignIn(pkceConfig, begin.pending, callbackUrl);
  assert.equal(complete.kind, 'exchange');
  assert.equal(complete.url, 'https://auth.example.com/token');
  assert.equal(complete.contentType, 'application/x-www-form-urlencoded');
  assert.ok(complete.body.includes('grant_type=authorization_code'));
  assert.ok(complete.body.includes('code=auth-code-123'));
  assert.ok(complete.body.includes('code_verifier='));
  assert.ok(complete.body.includes('client_id=test-client-id'));

  const tokenBody = JSON.stringify({
    access_token: 'access-tok-xyz',
    refresh_token: 'refresh-tok-abc',
    expires_in: 3600,
    scope: 'openid profile',
    token_type: 'Bearer',
  });
  const parsed = wasm.parseTokenResponse(pkceConfig, tokenBody, now);
  assert.equal(parsed.accessToken, 'access-tok-xyz');
  assert.equal(parsed.refreshToken, 'refresh-tok-abc');
  assert.equal(parsed.expiresAt, now + 3600);
  assert.equal(parsed.scope, 'openid profile');
});

test('PKCE with a JSON exchange body and a key token field', { skip }, async () => {
  const wasm = await loadWasm();
  const now = 1700000000;
  const config = JSON.stringify({
    mode: 'loopback',
    flow: 'pkce',
    authorize_url:
      'https://keys.example.com/auth?callback_url={redirect_uri}&code_challenge={code_challenge}&code_challenge_method=S256&state={state}',
    token_url: 'https://keys.example.com/api/keys',
    client_id: 'app',
    redirect_uri: 'http://localhost:9998/callback',
    capture: { at: 'request', from: 'query', param: 'code' },
    exchange: {
      format: 'json',
      token_field: 'key',
      body: { code: '{value}', code_verifier: '{code_verifier}', code_challenge_method: 'S256' },
    },
  });
  const begin = wasm.beginSignIn(config, '', now, random32, random16);
  const challenge = new URL(begin.authorizeUrl).searchParams.get('code_challenge');
  const complete = wasm.completeSignIn(
    config,
    begin.pending,
    `http://localhost:9998/callback?code=code-1&state=${encodeURIComponent(begin.state)}`
  );
  assert.equal(complete.kind, 'exchange');
  assert.equal(complete.url, 'https://keys.example.com/api/keys');
  assert.equal(complete.contentType, 'application/json');
  const sent = JSON.parse(complete.body);
  assert.deepEqual(Object.keys(sent).sort(), ['code', 'code_challenge_method', 'code_verifier']);
  assert.equal(sent.code, 'code-1');
  assert.equal(createHash('sha256').update(sent.code_verifier).digest('base64url'), challenge);

  const parsed = wasm.parseTokenResponse(config, JSON.stringify({ key: 'sk-FAKE-key-1' }), now);
  assert.equal(parsed.accessToken, 'sk-FAKE-key-1');
  assert.equal(parsed.expiresAt, undefined);
  assert.equal(parsed.refreshToken, undefined);
});

test('a configured exchange sends the scope passed to beginSignIn', { skip }, async () => {
  const wasm = await loadWasm();
  const config = JSON.stringify({
    mode: 'loopback',
    flow: 'pkce',
    authorize_url: 'https://keys.example.com/auth?scope={scope}&c={code_challenge}&state={state}',
    token_url: 'https://keys.example.com/api/keys',
    client_id: 'app',
    redirect_uri: 'http://localhost:9998/callback',
    scope: 'default',
    capture: { at: 'request', from: 'query', param: 'code' },
    exchange: { format: 'json', body: { code: '{value}', scope: '{scope}' } },
  });
  const begin = wasm.beginSignIn(config, 'wider', 1700000000, random32, random16);
  assert.equal(new URL(begin.authorizeUrl).searchParams.get('scope'), 'wider');
  const complete = wasm.completeSignIn(
    config,
    begin.pending,
    `http://localhost:9998/callback?code=c&state=${encodeURIComponent(begin.state)}`
  );
  assert.equal(JSON.parse(complete.body).scope, 'wider');
});

test('implicit flow with relay (Adobe-shaped)', { skip }, async () => {
  const wasm = await loadWasm();
  const now = 1700000000;
  const begin = wasm.beginSignIn(implicitRelayConfig, '', now, random32, random16);
  assert.ok(begin.authorizeUrl.startsWith('https://ims-na1.adobelogin.com'));
  assert.ok(begin.state);
  assert.ok(begin.expectedNonce);

  const stateDecoded = JSON.parse(atob(begin.state));
  assert.equal(stateDecoded.source, 'origin');
  assert.equal(stateDecoded.origin, 'https://test.sliccy.ai');
  assert.equal(stateDecoded.nonce, begin.expectedNonce);

  const fragment = `access_token=ims-token-456&expires_in=7200&scope=openid+AdobeID&state=${encodeURIComponent(begin.state)}`;
  const callbackUrl = `https://test.sliccy.ai/auth/callback?nonce=${begin.expectedNonce}#${fragment}`;
  const complete = wasm.completeSignIn(implicitRelayConfig, begin.pending, callbackUrl);
  assert.equal(complete.kind, 'token');
  assert.equal(complete.token, 'ims-token-456');
  assert.equal(complete.expiresAt, now + 7200);
  assert.equal(complete.scope, 'openid AdobeID');
});

test('refreshRequest', { skip }, async () => {
  const wasm = await loadWasm();
  const result = wasm.refreshRequest(pkceConfig, 'my-refresh-token');
  assert.equal(result.url, 'https://auth.example.com/token');
  assert.ok(result.body.includes('grant_type=refresh_token'));
  assert.ok(result.body.includes('refresh_token=my-refresh-token'));
  assert.ok(result.body.includes('client_id=test-client-id'));
});

test('scopesSatisfied', { skip }, async () => {
  const wasm = await loadWasm();
  assert.equal(wasm.scopesSatisfied('openid profile email', 'openid'), true);
  assert.equal(wasm.scopesSatisfied('openid', 'openid profile'), false);
  assert.equal(wasm.scopesSatisfied('repo', 'public_repo'), true);
});

test('shouldBackoff', { skip }, async () => {
  const wasm = await loadWasm();
  assert.equal(wasm.shouldBackoff(1700000000, 1700000100), true);
  assert.equal(wasm.shouldBackoff(1700000000, 1700000400), false);
});

test('completeSignIn handles IdP error', { skip }, async () => {
  const wasm = await loadWasm();
  const now = 1700000000;
  const begin = wasm.beginSignIn(pkceConfig, '', now, random32, random16);
  const errorUrl = `http://127.0.0.1:9999/callback?error=access_denied&error_description=User+denied&state=${encodeURIComponent(begin.state)}`;
  assert.throws(
    () => wasm.completeSignIn(pkceConfig, begin.pending, errorUrl),
    (err) =>
      err.kind === 'error' &&
      err.code === 'idp_error' &&
      err.error === 'access_denied' &&
      err.description === 'User denied'
  );
});

test('completeSignIn handles state mismatch', { skip }, async () => {
  const wasm = await loadWasm();
  const now = 1700000000;
  const begin = wasm.beginSignIn(pkceConfig, '', now, random32, random16);
  const badUrl = 'http://127.0.0.1:9999/callback?code=abc&state=wrong';
  assert.throws(
    () => wasm.completeSignIn(pkceConfig, begin.pending, badUrl),
    (err) => err.kind === 'error' && err.code === 'state_mismatch'
  );
});

test('parseTokenResponse handles IdP error', { skip }, async () => {
  const wasm = await loadWasm();
  const body = JSON.stringify({ error: 'invalid_grant', error_description: 'expired' });
  assert.throws(
    () => wasm.parseTokenResponse(pkceConfig, body, 1700000000),
    (err) =>
      err.kind === 'error' &&
      err.code === 'idp_error' &&
      err.error === 'invalid_grant' &&
      err.description === 'expired'
  );
  const bare = JSON.stringify({ error: 'invalid_grant' });
  assert.throws(
    () => wasm.parseTokenResponse(pkceConfig, bare, 1700000000),
    (err) =>
      err.error === 'invalid_grant' && !('description' in err) && err.message === 'invalid_grant'
  );
});

test('refreshTokenRequest builds a refresh with scope and resource, no sign-in config needed', {
  skip,
}, async () => {
  const wasm = await loadWasm();
  const result = wasm.refreshTokenRequest(
    JSON.stringify({
      token_url: 'https://as.example/token',
      client_id: 'c1',
      refresh_token: 'rt-secret',
      scope: 'read',
      resource: 'https://mcp.example/mcp',
    })
  );
  assert.equal(result.url, 'https://as.example/token');
  const body = new URLSearchParams(result.body);
  assert.equal(body.get('grant_type'), 'refresh_token');
  assert.equal(body.get('refresh_token'), 'rt-secret');
  assert.equal(body.get('scope'), 'read');
  assert.equal(body.get('resource'), 'https://mcp.example/mcp');
  for (const [params, code] of [
    [
      { token_url: 'http://as.example/token', client_id: 'c', refresh_token: 'rt-secret' },
      'invalid_params',
    ],
    [{ token_url: 'https://as.example/token', client_id: 'c' }, 'invalid_params'],
    [
      {
        token_url: 'https://as.example/token',
        client_id: 'c',
        refresh_token: 'rt-secret',
        extra: 'x',
      },
      'invalid_params',
    ],
  ]) {
    assert.throws(
      () => wasm.refreshTokenRequest(JSON.stringify(params)),
      (err) => err.code === code && !err.message.includes('rt-secret')
    );
  }
  assert.throws(
    () => wasm.refreshTokenRequest('"rt-secret"'),
    (err) => !err.message.includes('rt-secret')
  );
});

test('completeSignIn rejects wrong relay nonce', { skip }, async () => {
  const wasm = await loadWasm();
  const now = 1700000000;
  const begin = wasm.beginSignIn(implicitRelayConfig, '', now, random32, random16);
  const fragment = `access_token=tok&expires_in=3600&state=${encodeURIComponent(begin.state)}`;
  const callbackUrl = `https://test.sliccy.ai/auth/callback?nonce=wrong-nonce#${fragment}`;
  assert.throws(
    () => wasm.completeSignIn(implicitRelayConfig, begin.pending, callbackUrl),
    (err) => err.kind === 'error' && err.code === 'nonce_mismatch'
  );
});

test('completeSignIn rejects missing relay nonce', { skip }, async () => {
  const wasm = await loadWasm();
  const now = 1700000000;
  const begin = wasm.beginSignIn(implicitRelayConfig, '', now, random32, random16);
  const fragment = `access_token=tok&expires_in=3600&state=${encodeURIComponent(begin.state)}`;
  const callbackUrl = `https://test.sliccy.ai/auth/callback#${fragment}`;
  assert.throws(
    () => wasm.completeSignIn(implicitRelayConfig, begin.pending, callbackUrl),
    (err) => err.kind === 'error' && err.code === 'nonce_mismatch'
  );
});

test('implicit flow defaults to expires_in param', { skip }, async () => {
  const configNoExpiresParam = JSON.stringify({
    mode: 'popup',
    flow: 'implicit',
    authorize_url:
      'https://example.com/auth?client_id={client_id}&response_type=token&redirect_uri={redirect_uri}&state={state}',
    client_id: 'test',
    redirect_uri: 'http://127.0.0.1:8080/callback',
    capture: { at: 'callback', from: 'fragment', param: 'access_token' },
  });
  const wasm = await loadWasm();
  const now = 1700000000;
  const begin = wasm.beginSignIn(configNoExpiresParam, '', now, random32, random16);
  const fragment = `access_token=tok&expires_in=1800&state=${encodeURIComponent(begin.state)}`;
  const callbackUrl = `http://127.0.0.1:8080/callback#${fragment}`;
  const complete = wasm.completeSignIn(configNoExpiresParam, begin.pending, callbackUrl);
  assert.equal(complete.kind, 'token');
  assert.equal(complete.expiresAt, now + 1800);
});

test('parseRefreshResponse needs no config and validates like parseTokenResponse', {
  skip,
}, async () => {
  const wasm = await loadWasm();
  const ok = wasm.parseRefreshResponse(
    JSON.stringify({
      access_token: 'at-new',
      token_type: 'bearer',
      expires_in: 3600,
      refresh_token: 'rt-new',
      scope: 'read',
    }),
    1700000000
  );
  assert.equal(ok.accessToken, 'at-new');
  assert.equal(ok.refreshToken, 'rt-new');
  assert.equal(ok.expiresAt, 1700003600);
  assert.equal(ok.scope, 'read');
  const noRotation = wasm.parseRefreshResponse(
    JSON.stringify({ access_token: 'at', token_type: 'Bearer' }),
    1700000000
  );
  assert.equal(noRotation.refreshToken, undefined);
  for (const [body, code] of [
    [{ access_token: 'at' }, 'invalid_token_type'],
    [{ access_token: 'at', token_type: 'mac' }, 'invalid_token_type'],
    [{ access_token: '', token_type: 'Bearer' }, 'empty_access_token'],
    [{ access_token: 'at', token_type: 'Bearer', expires_in: '3600' }, 'invalid_expires_in'],
    [{ access_token: 'at', token_type: 'Bearer', error: 'x' }, 'error_with_token'],
  ]) {
    assert.throws(
      () => wasm.parseRefreshResponse(JSON.stringify(body), 1700000000),
      (err) => err.code === code,
      code
    );
  }
  assert.throws(
    () => wasm.parseRefreshResponse('"rt-secret-in-a-string"', 1700000000),
    (err) => err.code === 'invalid_json' && !err.message.includes('rt-secret')
  );
  assert.throws(
    () =>
      wasm.parseRefreshResponse(
        JSON.stringify({ error: 'invalid_grant', error_description: 'revoked' }),
        1700000000
      ),
    (err) =>
      err.code === 'idp_error' && err.error === 'invalid_grant' && err.description === 'revoked'
  );
});

test('refreshTokenRequest never echoes a URL, and returns the parsed URL', { skip }, async () => {
  const wasm = await loadWasm();
  const long = `evil${'x'.repeat(2000)}.example`;
  for (const token_url of [`http://${long}/t`, 'data://x/t', 'javascript://x/t']) {
    assert.throws(
      () =>
        wasm.refreshTokenRequest(
          JSON.stringify({ token_url, client_id: 'c', refresh_token: 'rt' })
        ),
      (err) =>
        err.code === 'invalid_params' &&
        err.message === 'token_url must use https (http only for loopback)' &&
        !err.message.includes('xxxx')
    );
  }
  assert.throws(
    () =>
      wasm.refreshTokenRequest(
        JSON.stringify({
          token_url: 'https://as.example/t',
          client_id: 'c',
          refresh_token: 'rt',
          resource: `http://${long}/`,
        })
      ),
    (err) => err.message === 'resource must use https (http only for loopback)'
  );
  const parsed = wasm.refreshTokenRequest(
    JSON.stringify({ token_url: 'https://:@AS.example/t', client_id: 'c', refresh_token: 'rt' })
  );
  assert.equal(parsed.url, 'https://as.example/t');
  const config = wasm.parseConfig(
    JSON.stringify({ ...JSON.parse(pkceConfig), token_url: `http://${long}/t` })
  );
  assert.equal(config.ok, false);
  assert.ok(!JSON.stringify(config.errors).includes('xxxx'));
});

test('refreshTokenRequestBytes zeroes the caller buffer', { skip }, async () => {
  const wasm = await loadWasm();
  const bytes = new TextEncoder().encode(
    JSON.stringify({ token_url: 'https://as.example/t', client_id: 'c', refresh_token: 'rt-bytes' })
  );
  const result = wasm.refreshTokenRequestBytes(bytes);
  assert.match(result.body, /refresh_token=rt-bytes/);
  assert.ok(bytes.every((b) => b === 0));
  const bad = new Uint8Array([0xff, 0xfe]);
  assert.throws(
    () => wasm.refreshTokenRequestBytes(bad),
    (err) => err.code === 'invalid_params'
  );
  assert.ok(bad.every((b) => b === 0));
});

test('a bad clock is an error, not a silent missing expiry', { skip }, async () => {
  const wasm = await loadWasm();
  const body = JSON.stringify({ access_token: 'at', token_type: 'Bearer', expires_in: 60 });
  for (const now of [Number.NaN, Number.POSITIVE_INFINITY, -1, 1.5, 2 ** 60]) {
    assert.throws(
      () => wasm.parseRefreshResponse(body, now),
      (err) => err.code === 'invalid_clock',
      String(now)
    );
    assert.throws(
      () => wasm.parseTokenResponse(pkceConfig, body, now),
      (err) => err.code === 'invalid_clock',
      String(now)
    );
    assert.throws(
      () => wasm.beginSignIn(pkceConfig, '', now, random32, random16),
      (err) => err.code === 'invalid_clock',
      String(now)
    );
  }
  assert.equal(wasm.parseRefreshResponse(body, 1700000000).expiresAt, 1700000060);
  const req = wasm.refreshTokenRequest(
    JSON.stringify({
      token_url: 'https://as.example/t',
      client_id: 'c',
      refresh_token: 'rt',
      resource: 'https://:@API.example/x',
    })
  );
  assert.equal(new URLSearchParams(req.body).get('resource'), 'https://api.example/x');
});
