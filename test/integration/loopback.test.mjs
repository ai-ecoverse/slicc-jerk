import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const loopback = { skip: process.platform === 'win32' && 'the system browser is rundll32' };

function freePort() {
  return new Promise((resolve) => {
    const s = createServer();
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

test(
  'the loopback listener waits for a callback that connects before it sends',
  loopback,
  async () => {
    const sb = sandbox();
    const port = await freePort();
    sb.writeUser(`[providers.slowcb]
env = ["SLOWCB_TOKEN"]
[providers.slowcb.oauth]
mode = "loopback"
flow = "implicit"
authorize_url = "https://app.example/auth?redirect_uri={redirect_uri}&state={state}"
client_id = "app-id"
redirect_uri = "http://127.0.0.1:${port}/cb"
capture = { at = "request", from = "query", param = "access_token" }
timeout = "10s"
`);
    const late = join(sb.dir, 'late.js');
    writeFileSync(
      late,
      `const u = new URL(process.argv[2]);
const back = new URL(u.searchParams.get('redirect_uri'));
const sock = require('node:net').connect(Number(back.port), '127.0.0.1', () => {
  setTimeout(() => {
    sock.write('GET /cb?access_token=slow-FAKE-0123456789&state=' + encodeURIComponent(u.searchParams.get('state')) + ' HTTP/1.1\\r\\nHost: 127.0.0.1\\r\\nConnection: close\\r\\n\\r\\n');
  }, 500);
});
sock.on('data', () => {});
sock.on('error', () => {});`
    );
    const browser = `require('node:child_process').spawn(process.execPath, [${JSON.stringify(late)}, args[0]], { detached: true, stdio: 'ignore' }).unref();`;
    sb.script('open', browser);
    sb.script('xdg-open', browser);
    const env = { SLICC_JERK_STORE: 'file' };
    const created = await sb.jerkAsync(['slowcb', 'create'], env);
    assert.equal(created.status, 0, created.stderr);
    const got = await sb.jerkAsync(['slowcb', 'get', '--reveal'], env);
    assert.equal(got.stdout, 'slow-FAKE-0123456789\n');
  }
);
