import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';
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
