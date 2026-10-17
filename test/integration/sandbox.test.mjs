import assert from 'node:assert/strict';
import { existsSync, readdirSync } from 'node:fs';
import { createServer } from 'node:http';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

let dir;

test('a token jerk stores in a sandbox is there while its test runs', async () => {
  const sb = sandbox();
  dir = sb.dir;
  const server = createServer((req, res) => {
    req.resume();
    req.on('end', () => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(
        JSON.stringify({
          access_token: 'sandboxAccessFAKE1234567890',
          token_type: 'bearer',
          expires_in: 3600,
        })
      );
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    sb.writeUser(`[providers.sandboxed]
env = ["SANDBOXED_TOKEN"]
store = "file"
[providers.sandboxed.oauth]
mode = "client_credentials"
token_url = "http://127.0.0.1:${server.address().port}/token"
client_id = "cid"
client_secret = "secret"
`);
    const got = await sb.jerkAsync(['sandboxed', 'get', '--reveal']);
    assert.equal(got.status, 0, got.stderr);
    assert.equal(got.stdout, 'sandboxAccessFAKE1234567890\n');
    assert.ok(readdirSync(sb.stateDir, { recursive: true }).length > 0);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test('the sandbox and the token in it are gone once that test has ended', () => {
  assert.ok(dir);
  assert.equal(existsSync(dir), false);
});
