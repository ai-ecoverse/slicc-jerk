import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = 'eyJhbGciOiJIUzI1NiJ9.fireflyFAKEFAKE.sig';

function spawnStub(script) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ['--input-type=module', '-e', script], {
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let out = '';
    const onData = (chunk) => {
      out += chunk;
      const line = out.split('\n').find((l) => l.startsWith('PORT='));
      if (!line) return;
      child.stdout.off('data', onData);
      const port = Number(line.slice(5));
      resolve({
        port,
        url: `http://127.0.0.1:${port}/ims/token/v3`,
        close: () =>
          new Promise((r) => {
            child.kill('SIGTERM');
            child.on('exit', () => r());
            setTimeout(r, 1000);
          }),
      });
    };
    child.stdout.on('data', onData);
    child.stderr.on('data', (c) => {
      out += c;
    });
    child.on('error', reject);
    child.on('exit', (code) => {
      if (code && code !== 0) reject(new Error(`stub exited ${code}: ${out}`));
    });
  });
}

function fireflyConfig(tokenUrl) {
  return `[providers.firefly]
name = "Adobe Firefly Services"
env = ["FIREFLY_ACCESS_TOKEN"]
domains = ["firefly-api.adobe.io"]
store = "file"
params = { client_id = "ff-client", client_secret = "ff-secret", org_id = "org@AdobeOrg", scope = "openid,AdobeID,firefly_api" }
[providers.firefly.oauth]
mode = "client_credentials"
token_url = ${JSON.stringify(tokenUrl)}
client_id = "{client_id}"
client_secret = "{client_secret}"
scope = "{scope}"
`;
}

test('firefly create mints a client_credentials token and get returns it', async () => {
  const sb = sandbox();
  const postsFile = `${sb.dir}/posts.json`;
  writeFileSync(postsFile, '[]');
  const stub = await spawnStub(`
import { createServer } from 'node:http';
import { readFileSync, writeFileSync } from 'node:fs';
const postsFile = ${JSON.stringify(postsFile)};
const server = createServer((req, res) => {
  let body = '';
  req.on('data', (c) => { body += c; });
  req.on('end', () => {
    const posts = JSON.parse(readFileSync(postsFile, 'utf8'));
    posts.push(body);
    writeFileSync(postsFile, JSON.stringify(posts));
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({
      access_token: ${JSON.stringify(TOKEN)},
      token_type: 'bearer',
      expires_in: 3600,
    }));
  });
});
server.listen(0, '127.0.0.1', () => {
  process.stdout.write('PORT=' + server.address().port + '\\n');
});
`);
  try {
    sb.writeUser(fireflyConfig(stub.url));
    const missing = sb.jerk(['firefly', 'get']);
    assert.equal(missing.status, 3);
    assert.match(missing.stderr, /create/);
    const created = sb.jerk(['firefly', 'create']);
    assert.equal(created.status, 0, created.stderr);
    assert.equal(created.stdout, 'firefly: signed in\n');
    const posts = JSON.parse(readFileSync(postsFile, 'utf8'));
    assert.equal(posts.length, 1);
    assert.match(posts[0], /grant_type=client_credentials/);
    assert.match(posts[0], /client_id=ff-client/);
    assert.match(posts[0], /client_secret=ff-secret/);
    assert.match(posts[0], /scope=openid%2CAdobeID%2Cfirefly_api/);
    const got = sb.jerk(['firefly', 'get', '--reveal']);
    assert.equal(got.status, 0, got.stderr);
    assert.equal(got.stdout, `${TOKEN}\n`);
    assert.equal(JSON.parse(readFileSync(postsFile, 'utf8')).length, 1);
    const json = JSON.parse(sb.jerk(['firefly', 'get', '--json']).stdout);
    assert.equal(json.source, 'oauth');
    assert.ok(json.masked.includes('…'));
  } finally {
    await stub.close();
  }
});

test('firefly create requires client credentials', () => {
  const sb = sandbox();
  sb.writeUser(`[providers.firefly]
env = ["FIREFLY_ACCESS_TOKEN"]
store = "file"
params = { client_id = "", client_secret = "", scope = "openid" }
[providers.firefly.oauth]
mode = "client_credentials"
token_url = "http://127.0.0.1:9/token"
client_id = "{client_id}"
client_secret = "{client_secret}"
scope = "{scope}"
`);
  const res = sb.jerk(['firefly', 'create']);
  assert.equal(res.status, 2);
  assert.match(res.stderr, /client_id is required/);
});

test('firefly get remints when the cached token is stale', async () => {
  const sb = sandbox();
  const countFile = `${sb.dir}/count.txt`;
  writeFileSync(countFile, '0');
  const stub = await spawnStub(`
import { createServer } from 'node:http';
import { readFileSync, writeFileSync } from 'node:fs';
const countFile = ${JSON.stringify(countFile)};
const server = createServer((req, res) => {
  let body = '';
  req.on('data', (c) => { body += c; });
  req.on('end', () => {
    const n = Number(readFileSync(countFile, 'utf8')) + 1;
    writeFileSync(countFile, String(n));
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({
      access_token: ${JSON.stringify(`${TOKEN}.`)} + n,
      token_type: 'Bearer',
      expires_in: 1,
    }));
  });
});
server.listen(0, '127.0.0.1', () => {
  process.stdout.write('PORT=' + server.address().port + '\\n');
});
`);
  try {
    sb.writeUser(fireflyConfig(stub.url));
    assert.equal(sb.jerk(['firefly', 'create']).status, 0);
    assert.equal(Number(readFileSync(countFile, 'utf8')), 1);
    const first = sb.jerk(['firefly', 'get', '--reveal']);
    assert.equal(first.status, 0, first.stderr);
    assert.equal(first.stdout, `${TOKEN}.2\n`);
    assert.equal(Number(readFileSync(countFile, 'utf8')), 2);
  } finally {
    await stub.close();
  }
});

test('built-in firefly shows client_credentials IMS config', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'firefly']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /mode = "client_credentials"/);
  assert.match(shown.stdout, /ims-na1\.adobelogin\.com\/ims\/token\/v3/);
  assert.match(shown.stdout, /FIREFLY_ACCESS_TOKEN/);
  assert.match(shown.stdout, /firefly_api/);
});
