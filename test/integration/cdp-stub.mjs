import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { runInNewContext } from 'node:vm';

export function listen(handler) {
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

export function stubCdp({ paused }) {
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
        paused.forEach((params, i) => {
          send({
            method: 'Fetch.requestPaused',
            sessionId,
            params: { requestId: `REQ${i + 1}`, frameId: 'F1', networkId: `N${i + 1}`, ...params },
          });
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
      resolve({
        wsUrl: `ws://127.0.0.1:${port}/devtools/browser`,
        log: () => lines.join('\n'),
        close: () =>
          new Promise((res, rej) => {
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
  return { opcode: buf[0] & 0x0f, payload, rest: buf.subarray(offset + maskLen + len) };
}

export function stubBrowser({ pages = [], cookies = [], storage = {} }) {
  return new Promise((resolve, reject) => {
    const sockets = new Set();
    const state = {
      pages: pages.map((p) => ({ type: 'page', title: '', ...p })),
      cookies: [...cookies],
      setCookies: [],
      storage: structuredClone(storage),
      calls: [],
      navigations: [],
      redirects: {},
    };
    let nextTarget = 1;
    const server = createServer((req, res) => {
      if (req.url === '/json/version') {
        const { port } = server.address();
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(
          JSON.stringify({ webSocketDebuggerUrl: `ws://127.0.0.1:${port}/devtools/browser/stub` })
        );
        return;
      }
      res.writeHead(404);
      res.end();
    });
    const origin = (url) => {
      try {
        return new URL(url).origin;
      } catch {
        return 'null';
      }
    };
    const store = (url, kind) => {
      const key = origin(url);
      state.storage[key] ??= { localStorage: {}, sessionStorage: {} };
      const values = state.storage[key][kind];
      return {
        setItem: (k, v) => {
          values[k] = String(v);
        },
      };
    };
    server.on('upgrade', (req, socket, head) => {
      if (!acceptWebSocket(req, socket, head)) {
        socket.destroy();
        return;
      }
      sockets.add(socket);
      const sessions = new Map();
      let nextSession = 1;
      let buf = Buffer.alloc(0);
      const send = (obj) => socket.write(wsFrame(JSON.stringify(obj)));
      const page = (id) => state.pages.find((p) => p.targetId === id);
      const handle = (msg) => {
        const { id, method, params = {}, sessionId } = msg;
        const reply = (result) => send({ id, result, ...(sessionId ? { sessionId } : {}) });
        const target = sessionId ? page(sessions.get(sessionId)) : null;
        state.calls.push(method);
        switch (method) {
          case 'Target.getTargets':
            return reply({ targetInfos: state.pages });
          case 'Target.getTargetInfo':
            return reply({ targetInfo: page(params.targetId) ?? { targetId: params.targetId } });
          case 'Target.createTarget': {
            const created = { targetId: `NEW${nextTarget++}`, type: 'page', url: 'about:blank' };
            state.pages.push(created);
            return reply({ targetId: created.targetId });
          }
          case 'Target.attachToTarget': {
            const sid = `S${nextSession++}`;
            sessions.set(sid, params.targetId);
            return reply({ sessionId: sid });
          }
          case 'Runtime.evaluate': {
            if (params.expression === 'location.href')
              return reply({ result: { value: target.url } });
            if (params.expression.includes('sessionStorage: session')) {
              const kept = state.storage[origin(target.url)] ?? {};
              return reply({
                result: {
                  value: {
                    url: target.url,
                    origin: origin(target.url),
                    title: target.title,
                    localStorage: kept.localStorage ?? {},
                    sessionStorage: kept.sessionStorage ?? {},
                  },
                },
              });
            }
            if (params.expression.includes('setItem')) {
              const value = runInNewContext(params.expression, {
                location: { origin: origin(target.url), href: target.url },
                localStorage: store(target.url, 'localStorage'),
                sessionStorage: store(target.url, 'sessionStorage'),
              });
              return reply({ result: { value: JSON.parse(JSON.stringify(value)) } });
            }
            return reply({ result: { value: 'complete' } });
          }
          case 'Network.getCookies':
            return reply({ cookies: state.cookies });
          case 'Network.setCookies':
            state.setCookies.push(...params.cookies);
            return reply({});
          case 'Page.navigate': {
            state.navigations.push(params.url);
            target.url = state.redirects[params.url] ?? state.redirect ?? params.url;
            reply({ frameId: 'F1' });
            send({
              method: 'Page.frameNavigated',
              sessionId,
              params: { frame: { id: 'F1', url: target.url } },
            });
            send({ method: 'Page.loadEventFired', sessionId, params: {} });
            return;
          }
          default:
            if (id != null) reply({});
        }
      };
      socket.on('data', (chunk) => {
        buf = Buffer.concat([buf, chunk]);
        while (true) {
          const parsed = readWsFrame(buf);
          if (!parsed) break;
          buf = parsed.rest;
          if (parsed.opcode === 8) {
            socket.end();
            return;
          }
          if (parsed.opcode !== 1) continue;
          handle(JSON.parse(parsed.payload.toString()));
        }
      });
      socket.on('close', () => sockets.delete(socket));
      socket.on('error', () => {});
    });
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      resolve({
        port,
        wsUrl: `ws://127.0.0.1:${port}/devtools/browser/stub`,
        state,
        close: () =>
          new Promise((res, rej) => {
            for (const s of sockets) s.destroy();
            server.close((err) => (err ? rej(err) : res()));
          }),
      });
    });
    server.on('error', reject);
  });
}
