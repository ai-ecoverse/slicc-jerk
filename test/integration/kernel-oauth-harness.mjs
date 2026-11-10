import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createNodeKernel } from '@ai-ecoverse/slicc-kernel/node';
import { wasm } from './helpers.mjs';

export const placeholder = 'fnox_proxy_333333333333333333333333';
export const fake = 'access-FAKE-1234567890';
export const refresh = 'refresh-FAKE-1234567890';
export const callback = 'http://127.0.0.1:43819/callback';

function bridge(wsUrl) {
  return async () => {
    const ws = new WebSocket(wsUrl);
    await new Promise((resolve, reject) => {
      ws.addEventListener('open', resolve, { once: true });
      ws.addEventListener('error', reject, { once: true });
    });
    const conn = {
      onmessage: null,
      onclose: null,
      send: (s) => ws.send(s),
      close: () => ws.close(),
    };
    ws.addEventListener('message', (event) => conn.onmessage?.(String(event.data)));
    ws.addEventListener('close', () => conn.onclose?.());
    return conn;
  };
}

export async function seven(mode, options = {}) {
  const posts = [];
  const seals = [];
  const requests = [];
  const records = new Map(Object.entries(options.records ?? {}));
  let polls = 0;
  const kernel = await createNodeKernel({
    users: 'enforce',
    ...(options.browser ? { cdp: bridge(options.browser.wsUrl) } : {}),
    sealed: async (req) => {
      requests.push(req);
      if (options.sealed) return options.sealed(req);
      const overridden = options.reply?.(req, records);
      if (overridden !== undefined) return overridden;
      if (req.op === 'capabilities')
        return { v: 1, capabilities: options.capabilities ?? { kernelOAuth: 1 } };
      if (req.op === 'oauthGrant') {
        seals.push(req);
        if (options.refused) return { v: 1, error: { code: 'EEXIST', message: fake } };
        const state = options.expired ? 'refreshing' : 'ready';
        const status = {
          v: 1,
          placeholder,
          state,
          expiresAt: req.expiresAt ?? null,
          refreshable: req.refreshToken !== undefined,
        };
        records.set(req.name, { ...status, oauthManaged: true });
        return status;
      }
      if (req.op === 'oauthStatus' || req.op === 'oauthRefresh') {
        const record = records.get(req.name);
        if (!record?.oauthManaged) return { v: 1, error: { code: 'ENOENT', message: 'absent' } };
        if (req.op === 'oauthRefresh' && !options.expired) record.expiresAt = Date.now() + 3600000;
        const { oauthManaged, ...status } = record;
        return status;
      }
      if (req.op === 'oauthRemove') {
        if (options.dropDenied) return { v: 1, error: { code: 'EACCES', message: fake } };
        records.delete(req.name);
        return { v: 1, removed: true };
      }
      if (req.kind === 'unseal') {
        if (!options.dropDenied) records.clear();
        return { v: 1, exit: options.dropDenied ? 1 : 0 };
      }
      if (req.jop === 'status') return { v: 1, exit: 3 };
      if (req.op === 'placeholder')
        return records.has(req.name)
          ? { v: 1, ...records.get(req.name) }
          : { v: 1, error: { code: 'ENOENT' } };
      assert.equal(req.op, 'seal');
      seals.push(req);
      if (options.refused) return { v: 1, error: { code: 'EEXIST', message: fake } };
      records.set(req.name, { placeholder, state: 'inert' });
      return { v: 1, placeholder, state: 'inert' };
    },
    network: {
      transport: {
        traits: { manualRedirects: true, encodedBodies: false, maxRequestBody: 1024 * 1024 },
        async fetch(req) {
          posts.push({
            url: req.url,
            method: req.method,
            body: new TextDecoder().decode(req.body),
          });
          let response;
          let status = 200;
          if (req.url.endsWith('/device')) {
            response = {
              device_code: 'device-FAKE',
              user_code: 'CODE-FAKE',
              verification_uri: 'https://idp.example.test/verify',
              expires_in: options.pending ? 3 : 30,
              interval: 0,
            };
          } else if (options.denied) {
            status = 400;
            response = { error: 'access_denied', error_description: fake };
          } else if (mode === 'device' && (options.pending || polls++ === 0)) {
            status = 400;
            response = { error: 'authorization_pending' };
          } else {
            response = {
              access_token: fake,
              refresh_token: options.noRefresh ? undefined : refresh,
              token_type: 'Bearer',
              expires_in: options.noExpiry ? undefined : options.expired ? 0 : 3600,
              scope: options.secretScope ? refresh : undefined,
            };
          }
          return {
            status,
            statusText: 'stub',
            headers: [['content-type', 'application/json']],
            body: (async function* () {
              yield new TextEncoder().encode(JSON.stringify(response));
            })(),
            cancel: async () => {},
          };
        },
      },
    },
  });
  const pkg = '/node_modules/@ai-ecoverse/slicc-jerk';
  await kernel.writeFile(
    `${pkg}/package.json`,
    JSON.stringify({
      name: '@ai-ecoverse/slicc-jerk',
      slicc: {
        abi: 'wasi',
        commands: { jerk: { wasm: 'jerk.wasm', imports: 'kernel-process.mjs' } },
      },
    })
  );
  await kernel.writeFile(`${pkg}/jerk.wasm`, readFileSync(wasm));
  await kernel.writeFile(
    `${pkg}/kernel-process.mjs`,
    readFileSync(new URL('../../npm/kernel-process.mjs', import.meta.url))
  );
  if (options.cli || options.providerCli) {
    const command = options.providerCli ?? 'fake-minter';
    const minter = '/node_modules/fake-minter';
    await kernel.writeFile(
      `${minter}/package.json`,
      JSON.stringify({
        name: 'fake-minter',
        slicc: { abi: 'js', commands: { [command]: { module: 'minter.mjs' } } },
      })
    );
    await kernel.writeFile(
      `${minter}/minter.mjs`,
      `export default async function(ctx) {
        await ctx.fs.writeFile('/tmp/minter-call', 'called');
        const out = ctx.argv.includes('--version') ? '4.100.0' : ${options.providerCli ? `JSON.stringify({ token: '${fake}' })` : `'${fake}'`};
        await ctx.write(1, out); return 0;
      }`
    );
  }
  await kernel.writeFile(
    '/tmp/providers.toml',
    options.config ??
      `[providers.demo]
env = ["DEMO_TOKEN"]
${options.cli ? 'requires = "fake-minter"\ncreate = { run = ["fake-minter"], parse = "text" }' : ''}
[providers.demo.oauth]
mode = "${mode}"
flow = "pkce"
authorize_url = "https://idp.example.test/${mode === 'device' ? 'device' : 'authorize?redirect_uri={redirect_uri}&state={state}&code_challenge={code_challenge}'}"
token_url = "https://idp.example.test/token"
client_id = "fake-client"
redirect_uri = "${callback}"
capture = { at = "request", from = "query", param = "code" }
timeout = "3s"
`
  );
  const run = (...args) =>
    kernel.run(['jerk', options.provider ?? 'demo', ...args], {
      env: {
        SLICC_JERK_CONFIG: '/tmp/providers.toml',
        SLICC_JERK_STATE_DIR: '/tmp/oauth-state',
        ...options.env,
      },
    });
  return { kernel, posts, seals, run, requests, records };
}
