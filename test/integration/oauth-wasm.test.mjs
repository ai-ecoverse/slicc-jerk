import assert from 'node:assert/strict';
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
    (err) => err.kind === 'error' && err.code === 'idp_error'
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
    (err) => err.kind === 'error' && err.code === 'idp_error'
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
