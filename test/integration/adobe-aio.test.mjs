import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const ACCESS = 'eyJhbGciOiJIUzI1NiJ9.e30.s2s_fake_access_token_abcd';

function startTokenServer() {
  return new Promise((resolve, reject) => {
    const script = `
const http = require('node:http');
const bodies = [];
const server = http.createServer((req, res) => {
  const chunks = [];
  req.on('data', (c) => chunks.push(c));
  req.on('end', () => {
    bodies.push(Buffer.concat(chunks).toString());
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({
      access_token: ${JSON.stringify(ACCESS)},
      token_type: 'bearer',
      expires_in: 3600,
      scope: 'openid,AdobeID,aem.cdn',
    }));
  });
});
server.listen(0, '127.0.0.1', () => {
  process.stdout.write(JSON.stringify({ port: server.address().port }) + '\\n');
});
process.stdin.on('data', (buf) => {
  if (buf.toString().includes('bodies')) {
    process.stdout.write(JSON.stringify({ bodies }) + '\\n');
  }
  if (buf.toString().includes('close')) {
    server.close(() => process.exit(0));
  }
});
`;
    const child = spawn(process.execPath, ['-e', script], {
      stdio: ['pipe', 'pipe', 'inherit'],
    });
    let buf = '';
    const onData = (chunk) => {
      buf += chunk;
      const line = buf.indexOf('\n');
      if (line === -1) return;
      child.stdout.off('data', onData);
      try {
        const { port } = JSON.parse(buf.slice(0, line));
        buf = buf.slice(line + 1);
        resolve({
          url: `http://127.0.0.1:${port}/token`,
          bodies() {
            return new Promise((res, rej) => {
              const read = (chunk) => {
                buf += chunk;
                const i = buf.indexOf('\n');
                if (i === -1) return;
                child.stdout.off('data', read);
                try {
                  res(JSON.parse(buf.slice(0, i)).bodies);
                } catch (e) {
                  rej(e);
                }
              };
              child.stdout.on('data', read);
              child.stdin.write('bodies\n');
            });
          },
          async close() {
            child.stdin.write('close\n');
            await new Promise((r) => child.once('exit', r));
          },
        });
      } catch (e) {
        reject(e);
      }
    };
    child.stdout.on('data', onData);
    child.on('error', reject);
  });
}

function providerToml(tokenUrl) {
  return `[providers.adobe-aio]
env = ["ADOBE_IMS_TOKEN", "AEM_EDGE_FUNCTIONS_TOKEN"]
domains = ["ims-na1.adobelogin.com"]
store = "file"
params = { ctx = "cli", client_id = "", client_secret = "", scope = "openid,AdobeID,aem.cdn" }

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
token_url = ${JSON.stringify(tokenUrl)}
client_id = "{client_id}"
client_secret = "{client_secret}"
scope = "{scope}"
`;
}

test('adobe-aio get uses aio when client_id is unset', () => {
  const sb = sandbox();
  sb.writeUser(providerToml('http://127.0.0.1:1/token'));
  sb.script('aio', `process.stdout.write(${JSON.stringify(`${ACCESS}\n`)});`);
  const res = sb.jerk(['adobe-aio', 'get', '--reveal']);
  assert.equal(res.status, 0, res.stderr);
  assert.equal(res.stdout, `${ACCESS}\n`);
  assert.deepEqual(sb.calls()[0].args, ['login', '--bare', '--no-open', '--ctx', 'cli']);
});

test('adobe-aio get mints client_credentials when -p client_id is set', async () => {
  const srv = await startTokenServer();
  try {
    const sb = sandbox();
    sb.writeUser(providerToml(srv.url));
    sb.script('aio', 'process.exit(9);');
    const res = sb.jerk([
      'adobe-aio',
      'get',
      '--reveal',
      '-p',
      'client_id=adc_client',
      '-p',
      'client_secret=adc_secret',
    ]);
    assert.equal(res.status, 0, res.stderr);
    assert.equal(res.stdout, `${ACCESS}\n`);
    assert.equal(sb.calls().length, 0);
    const bodies = await srv.bodies();
    assert.equal(bodies.length, 1);
    const params = new URLSearchParams(bodies[0]);
    assert.equal(params.get('grant_type'), 'client_credentials');
    assert.equal(params.get('client_id'), 'adc_client');
    assert.equal(params.get('client_secret'), 'adc_secret');
    assert.equal(params.get('scope'), 'openid,AdobeID,aem.cdn');
    const again = sb.jerk([
      'adobe-aio',
      'get',
      '--json',
      '-p',
      'client_id=adc_client',
      '-p',
      'client_secret=adc_secret',
    ]);
    assert.equal(again.status, 0, again.stderr);
    assert.equal(JSON.parse(again.stdout).source, 'oauth');
    assert.equal((await srv.bodies()).length, 1);
  } finally {
    await srv.close();
  }
});

test('adobe-aio create with S2S params skips aio and stores the grant', async () => {
  const srv = await startTokenServer();
  try {
    const sb = sandbox();
    sb.writeUser(providerToml(srv.url));
    sb.script('aio', 'process.stderr.write("should not run\\n"); process.exit(9);');
    const created = sb.jerk([
      'adobe-aio',
      'create',
      '--force',
      '-p',
      'client_id=adc_client',
      '-p',
      'client_secret=adc_secret',
      '--scope',
      'aem.cdn',
    ]);
    assert.equal(created.status, 0, created.stderr);
    assert.equal(created.stdout, 'adobe-aio: signed in\n');
    assert.equal(sb.calls().length, 0);
    const got = sb.jerk([
      'adobe-aio',
      'get',
      '--reveal',
      '-p',
      'client_id=adc_client',
      '-p',
      'client_secret=adc_secret',
    ]);
    assert.equal(got.status, 0, got.stderr);
    assert.equal(got.stdout, `${ACCESS}\n`);
    const dropped = sb.jerk(['adobe-aio', 'drop', '--yes', '-p', 'client_id=adc_client']);
    assert.equal(dropped.status, 0, dropped.stderr);
  } finally {
    await srv.close();
  }
});

test('builtin adobe-aio exposes client_credentials oauth and chain gates', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'adobe-aio']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /mode = "client_credentials"/);
  assert.match(shown.stdout, /ims\/token\/v3/);
  assert.match(shown.stdout, /unless = \["client_id"\]/);
  assert.match(shown.stdout, /needs = \["client_id", "client_secret"\]/);
  assert.match(shown.stdout, /aem\.cdn/);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 0, check.stderr);
});
