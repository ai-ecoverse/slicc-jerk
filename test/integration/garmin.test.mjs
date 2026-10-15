import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

test('garmin is a built-in intercept exchange provider', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'garmin']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /mode = "intercept"/);
  assert.match(shown.stdout, /flow = "exchange"/);
  assert.match(shown.stdout, /param = "ticket"/);
  assert.match(shown.stdout, /GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2/);
  assert.match(shown.stdout, /service_ticket = "\{value\}"/);
  assert.match(shown.stdout, /diauth\.garmin\.com/);
  assert.match(shown.stdout, /connectapi\.garmin\.com/);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 0, check.stderr);
});

test('garmin get says not signed in without a grant or env token', () => {
  const sb = sandbox();
  const res = sb.jerk(['garmin', 'get']);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /sign in with jerk garmin create/);
});

test('garmin create needs a CDP browser', () => {
  const sb = sandbox();
  const res = sb.jerk(['garmin', 'create']);
  assert.equal(res.status, 4);
  assert.match(res.stderr, /CDP|cdp|browser/);
});

test('garmin create exchanges a captured ticket through stub CDP and token HTTP', async () => {
  const sb = sandbox();
  const posts = [];
  const http = await listen((req, res) => {
    if (req.method === 'POST' && req.url === '/oauth/token') {
      let body = '';
      req.on('data', (c) => {
        body += c;
      });
      req.on('end', () => {
        posts.push(body);
        const access = body.includes('grant_type=refresh_token')
          ? 'garminRefreshedAccessFAKE1234567890'
          : 'garminAccessTokenFAKE1234567890';
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(
          JSON.stringify({
            access_token: access,
            refresh_token: 'garminRefreshTokenFAKE1234567890',
            token_type: 'Bearer',
            expires_in: 93600,
          })
        );
      });
      return;
    }
    res.writeHead(404);
    res.end();
  });
  const cdp = await stubCdp({
    redirectUrl: 'https://connect.garmin.com/modern?ticket=ST-fake-ticket-123',
  });
  try {
    sb.writeUser(`[providers.garmin]
name = "Garmin Connect"
env = ["GARMIN_TOKEN"]
store = "file"
domains = ["connectapi.garmin.com"]
[providers.garmin.oauth]
mode = "intercept"
flow = "exchange"
authorize_url = "https://sso.example.invalid/signin"
token_url = "http://127.0.0.1:${http.port}/oauth/token"
client_id = "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2"
redirect_uri = "https://connect.garmin.com/modern"
capture = { at = "request", from = "query", param = "ticket" }
on_capture = "close"
timeout = "10s"
[providers.garmin.oauth.exchange]
grant_type = "https://connectapi.garmin.com/di-oauth2-service/oauth/grant/service_ticket"
body = { service_ticket = "{value}", service_url = "https://connect.garmin.com/modern" }
`);
    const created = await sb.jerkAsync(['garmin', 'create', '--force', '--cdp', cdp.wsUrl]);
    assert.equal(created.status, 0, `${created.stderr}\n--- cdp ---\n${cdp.log()}`);
    assert.match(created.stdout, /garmin: signed in/);
    assert.equal(posts.length, 1);
    assert.match(posts[0], /service_ticket=ST-fake-ticket-123/);
    assert.match(posts[0], /client_id=GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2/);
    assert.match(
      posts[0],
      /grant_type=https%3A%2F%2Fconnectapi\.garmin\.com%2Fdi-oauth2-service%2Foauth%2Fgrant%2Fservice_ticket/
    );

    const got = await sb.jerkAsync(['garmin', 'get', '--reveal']);
    assert.equal(got.status, 0, got.stderr);
    assert.match(got.stdout, /garminRefreshedAccessFAKE1234567890/);
    assert.equal(posts.length, 2);
    assert.match(posts[1], /grant_type=refresh_token/);
  } finally {
    await cdp.close();
    await http.close();
  }
});

function listen(handler) {
  return new Promise((resolve, reject) => {
    const server = createServer(handler);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      resolve({
        port,
        close: () => new Promise((res, rej) => server.close((err) => (err ? rej(err) : res()))),
      });
    });
    server.on('error', reject);
  });
}

function stubCdp({ redirectUrl }) {
  return new Promise((resolve, reject) => {
    const sockets = new Set();
    const lines = [];
    const log = (line) => lines.push(line);
    const server = createServer((req, res) => {
      res.writeHead(404);
      res.end();
    });
    server.on('upgrade', (req, socket, head) => {
      log(`upgrade ${req.url}`);
      if (!acceptWebSocket(req, socket, head)) {
        socket.destroy();
        return;
      }
      sockets.add(socket);
      let nextSession = 1;
      let navigateSession = null;
      let buf = Buffer.alloc(0);
      const send = (obj) => {
        log(`<< ${JSON.stringify(obj)}`);
        socket.write(wsFrame(JSON.stringify(obj)));
      };
      const onPaused = (sessionId) => {
        send({
          method: 'Fetch.requestPaused',
          sessionId,
          params: {
            requestId: 'REQ1',
            frameId: 'F1',
            resourceType: 'Document',
            networkId: 'N1',
            request: {
              url: redirectUrl,
              method: 'GET',
              headers: { Accept: 'text/html' },
            },
          },
        });
      };
      socket.on('data', (chunk) => {
        buf = Buffer.concat([buf, chunk]);
        while (true) {
          const parsed = readWsFrame(buf);
          if (!parsed) break;
          buf = parsed.rest;
          let msg;
          try {
            msg = JSON.parse(parsed.payload.toString());
          } catch (err) {
            log(`bad frame: ${err}`);
            continue;
          }
          log(`>> ${JSON.stringify(msg)}`);
          const id = msg.id;
          const method = msg.method;
          const sessionId = msg.sessionId;
          if (method === 'Target.createTarget') {
            send({ id, result: { targetId: 'T1' } });
          } else if (method === 'Target.getTargetInfo') {
            send({
              id,
              result: {
                targetInfo: {
                  targetId: msg.params?.targetId || 'T1',
                  type: 'page',
                  url: 'about:blank',
                  title: '',
                  attached: true,
                },
              },
            });
          } else if (method === 'Target.attachToTarget') {
            const sid = `SESS${nextSession++}`;
            send({ id, result: { sessionId: sid } });
          } else if (method === 'Target.setDiscoverTargets') {
            send({ id, result: {} });
          } else if (method === 'Page.navigate') {
            send({ id, result: { frameId: 'F1' }, sessionId });
            navigateSession = sessionId;
            setTimeout(() => onPaused(navigateSession), 20);
          } else if (
            method === 'Fetch.failRequest' ||
            method === 'Fetch.continueRequest' ||
            method === 'Fetch.fulfillRequest' ||
            method === 'Fetch.enable' ||
            method === 'Fetch.disable' ||
            method === 'Page.enable' ||
            method === 'Network.enable' ||
            method === 'Runtime.enable' ||
            method === 'Network.setBypassServiceWorker'
          ) {
            send({ id, result: {}, sessionId });
          } else if (method === 'Target.closeTarget' || method === 'Target.detachFromTarget') {
            send({ id, result: { success: true } });
          } else if (id != null) {
            send({ id, result: {}, sessionId });
          }
        }
      });
      socket.on('close', () => sockets.delete(socket));
      socket.on('error', (err) => log(`socket error: ${err}`));
    });
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      const logFile = join(tmpdir(), `jerk-cdp-stub-${port}.log`);
      resolve({
        wsUrl: `ws://127.0.0.1:${port}/devtools/browser`,
        logFile,
        log: () => {
          writeFileSync(logFile, lines.join('\n'));
          return lines.join('\n');
        },
        close: () =>
          new Promise((res, rej) => {
            writeFileSync(logFile, lines.join('\n'));
            for (const s of sockets) s.destroy();
            server.close((err) => (err ? rej(err) : res()));
          }),
      });
    });
    server.on('error', reject);
  });
}

function acceptWebSocket(req, socket, head) {
  const key = req.headers['sec-websocket-key'];
  if (!key) return false;
  const accept = createHash('sha1')
    .update(key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11')
    .digest('base64');
  socket.write(
    'HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ' +
      accept +
      '\r\n\r\n'
  );
  if (head?.length) socket.unshift(head);
  return true;
}

function wsFrame(text) {
  const payload = Buffer.from(text);
  if (payload.length < 126) {
    return Buffer.concat([Buffer.from([0x81, payload.length]), payload]);
  }
  const header = Buffer.alloc(4);
  header[0] = 0x81;
  header[1] = 126;
  header.writeUInt16BE(payload.length, 2);
  return Buffer.concat([header, payload]);
}

function readWsFrame(buf) {
  if (buf.length < 2) return null;
  const masked = (buf[1] & 0x80) !== 0;
  let len = buf[1] & 0x7f;
  let offset = 2;
  if (len === 126) {
    if (buf.length < 4) return null;
    len = buf.readUInt16BE(2);
    offset = 4;
  } else if (len === 127) {
    if (buf.length < 10) return null;
    len = Number(buf.readBigUInt64BE(2));
    offset = 10;
  }
  const maskLen = masked ? 4 : 0;
  if (buf.length < offset + maskLen + len) return null;
  let payload = buf.subarray(offset + maskLen, offset + maskLen + len);
  if (masked) {
    const mask = buf.subarray(offset, offset + 4);
    payload = Buffer.from(payload.map((b, i) => b ^ mask[i % 4]));
  }
  return { payload, rest: buf.subarray(offset + maskLen + len) };
}
