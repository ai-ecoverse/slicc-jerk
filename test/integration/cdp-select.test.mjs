import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { createServer, request } from 'node:http';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { stubBrowser, stubCdp } from './cdp-stub.mjs';
import { jerkAsync, sandbox } from './helpers.mjs';

const browsers = async (...names) => {
  const out = {};
  for (const name of names) {
    out[name] = await stubBrowser({
      pages: [{ targetId: `PAGE-${name}`, url: `https://${name}.example.test/`, title: name }],
    });
  }
  after(() => Promise.all(Object.values(out).map((b) => b.close().catch(() => {}))));
  return out;
};

const listed = async (sb, extra = {}, args = []) => {
  const res = await sb.jerkAsync(['tab', 'list', '--json', ...args], extra);
  assert.equal(res.status, 0, res.stderr);
  return { ids: JSON.parse(res.stdout).map((p) => p.id), stderr: res.stderr };
};

const entry = (browser, { tier, sevenOpen = false, profile, sevenTabs = [] }) => ({
  kind: 'endpoint',
  tier,
  endpoint: {
    cdp: `http://127.0.0.1:${browser.port}`,
    webSocketDebuggerUrl: browser.wsUrl,
    browser: 'Chrome',
    sevenOpen,
    ...(profile ? { profile } : {}),
    sevenTabs,
    tabs: [],
  },
});

const slicc = (sb, entries) =>
  sb.script(
    'slicc',
    `process.stdout.write(${JSON.stringify(JSON.stringify({ schema: 1, entries }))});`
  );

const activePort = (sb, browser) => {
  const config =
    process.platform === 'darwin'
      ? join(sb.env.HOME, 'Library', 'Application Support')
      : sb.env.XDG_CONFIG_HOME;
  const dir = join(config, 'slicc', 'seven-profiles', 'p1');
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'DevToolsActivePort'), `${browser.port}\n/devtools/browser/stub\n`);
};

const unsupported = (value) =>
  `unsupported CDP URL "${value}" (expected a port, host:port, ws://, wss://, http://, or https://; use --cdp auto to search for a running browser)`;

const via = (endpoint, rule) => `* CDP ${endpoint} (via ${rule})\n`;
const version = (b) => `http://127.0.0.1:${b.port}/json/version`;

test('--cdp accepts the playwright-cli forms and refuses the rest with the same words', async () => {
  const sb = sandbox();
  const { a } = await browsers('a');
  for (const [form, endpoint] of [
    [String(a.port), `http://localhost:${a.port}/json/version`],
    [`127.0.0.1:${a.port}`, version(a)],
    [`http://127.0.0.1:${a.port}`, version(a)],
    [`ws://127.0.0.1:${a.port}`, version(a)],
    [a.wsUrl, a.wsUrl],
  ]) {
    const { ids, stderr } = await listed(sb, {}, ['--cdp', form]);
    assert.deepEqual(ids, ['PAGE-a'], form);
    assert.equal(stderr, via(endpoint, '--cdp'));
  }
  for (const bad of ['0', 'localhost:0', 'user@host:9222', 'localhost:99999']) {
    const res = await sb.jerkAsync(['tab', 'list', '--cdp', bad]);
    assert.equal(res.status, 2, bad);
    assert.equal(res.stderr, `jerk tab: ${unsupported(bad)}\n`);
    assert.equal(res.stdout, '');
  }
  const typo = await sb.jerkAsync(['tab', 'list', '--cpd', '0']);
  assert.equal(typo.status, 2);
  assert.match(typo.stderr, /unknown option --cpd\nDid you mean --cdp\?\n/);
});

test('with several browsers running, the order is --cdp, SLICC_CDP_URL, slicc browsers, DevToolsActivePort, port probe', async () => {
  const sb = sandbox();
  const b = await browsers('flag', 'env', 'tier', 'profile', 'seven', 'port', 'probe');
  slicc(sb, [
    entry(b.tier, { tier: 1 }),
    entry(b.profile, { tier: 2, profile: 'work' }),
    entry(b.seven, { tier: 3, sevenOpen: true }),
  ]);
  activePort(sb, b.port);
  const probe = { SLICC_CDP_PORTS: String(b.probe.port) };
  const env = { ...probe, SLICC_CDP_URL: String(b.env.port) };

  let got = await listed(sb, env, ['--cdp', String(b.flag.port)]);
  assert.deepEqual(got.ids, ['PAGE-flag']);

  got = await listed(sb, env);
  assert.deepEqual(got.ids, ['PAGE-env']);
  assert.equal(got.stderr, via(`http://localhost:${b.env.port}/json/version`, 'SLICC_CDP_URL'));

  got = await listed(sb, env, ['--cdp', 'auto']);
  assert.deepEqual(got.ids, ['PAGE-seven']);
  assert.equal(got.stderr, via(version(b.seven), 'slicc browsers'));

  slicc(sb, [entry(b.tier, { tier: 1 }), entry(b.profile, { tier: 2, profile: 'work' })]);
  got = await listed(sb, probe);
  assert.deepEqual(got.ids, ['PAGE-profile']);

  slicc(sb, [entry(b.tier, { tier: 2 }), entry(b.env, { tier: 1 })]);
  got = await listed(sb, probe);
  assert.deepEqual(got.ids, ['PAGE-env']);

  slicc(sb, [
    {
      kind: 'endpoint',
      tier: 0,
      endpoint: {
        cdp: 'http://192.0.2.1:9222',
        webSocketDebuggerUrl: 'ws://192.0.2.1:9222/devtools/browser/x',
      },
    },
  ]);
  got = await listed(sb, probe);
  assert.deepEqual(got.ids, ['PAGE-port']);
  assert.equal(got.stderr, via(version(b.port), 'DevToolsActivePort'));

  sb.script('slicc', 'process.exit(1);');
  got = await listed(sb, probe);
  assert.deepEqual(got.ids, ['PAGE-port']);
});

const sessionFile = (sb, cwd) => {
  const config =
    process.platform === 'darwin'
      ? join(sb.env.HOME, 'Library', 'Application Support')
      : sb.env.XDG_CONFIG_HOME;
  const key = createHash('sha256').update(realpathSync(cwd)).digest('hex');
  const dir = join(config, 'slicc', 'playwright-cli', 'sessions');
  mkdirSync(dir, { recursive: true });
  return join(dir, `${key}.json`);
};

const inDir = (sb, cwd, args, extra = {}) =>
  jerkAsync(['tab', 'list', '--json', ...args], { env: { ...sb.env, ...extra }, cwd });

test('the endpoint playwright-cli saved for this directory comes before the port probe', async () => {
  const sb = sandbox();
  const b = await browsers('saved', 'probe');
  const probe = { SLICC_CDP_PORTS: String(b.probe.port) };
  const cwd = join(sb.dir, 'work');
  mkdirSync(cwd);

  let res = await inDir(sb, cwd, [], probe);
  assert.equal(res.status, 0, res.stderr);
  assert.deepEqual(
    JSON.parse(res.stdout).map((p) => p.id),
    ['PAGE-probe']
  );
  assert.equal(res.stderr, via(version(b.probe), 'port probe'));

  writeFileSync(
    sessionFile(sb, cwd),
    JSON.stringify({ endpoint: version(b.saved), provenance: 'probe' })
  );
  res = await inDir(sb, cwd, [], probe);
  assert.deepEqual(
    JSON.parse(res.stdout).map((p) => p.id),
    ['PAGE-saved']
  );
  assert.equal(res.stderr, via(version(b.saved), 'saved endpoint'));

  await b.saved.close();
  res = await inDir(sb, cwd, [], probe);
  assert.deepEqual(
    JSON.parse(res.stdout).map((p) => p.id),
    ['PAGE-probe']
  );
});

test('a non-loopback saved endpoint is never dialled', async () => {
  const sb = sandbox();
  const cwd = join(sb.dir, 'work');
  mkdirSync(cwd);
  writeFileSync(
    sessionFile(sb, cwd),
    JSON.stringify({ endpoint: 'http://192.0.2.1:9222/json/version', provenance: 'explicit' })
  );
  const res = await inDir(sb, cwd, []);
  assert.equal(res.status, 4);
  assert.match(
    res.stderr,
    /\n {2}saved endpoint {6}saved endpoint is not loopback or has no known provenance\n/
  );
});

test('when no browser is found, jerk lists what it tried', async () => {
  const sb = sandbox();
  const cwd = join(sb.dir, 'work');
  mkdirSync(cwd);
  const res = await inDir(sb, cwd, ['--cdp', 'auto'], { SLICC_CDP_URL: '9333' });
  assert.equal(res.status, 4);
  assert.equal(res.stdout, '');
  assert.equal(
    res.stderr,
    [
      'jerk tab: no CDP browser found. Tried, in order:',
      '  --cdp               auto',
      '  SLICC_CDP_URL       set, skipped by --cdp auto',
      '  slicc browsers      slicc not on PATH, or it failed',
      '  DevToolsActivePort  no live file under slicc seven-profiles, profiles, or six profiles',
      '  saved endpoint      none for this directory',
      '  port probe          nothing answered on 127.0.0.1:1',
      'Pass --cdp <port|host:port|ws://...|http://...>, set SLICC_CDP_URL, or start a browser with --remote-debugging-port.',
      '',
    ].join('\n')
  );
});

const freePort = () =>
  new Promise((resolve) => {
    const s = createServer();
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });

const loopbackProvider = async (sb, timeout = '10s', mode = 'loopback') => {
  const port = await freePort();
  sb.writeUser(`[providers.lb]
env = ["LB_TOKEN"]
[providers.lb.oauth]
mode = "${mode}"
flow = "implicit"
authorize_url = "https://idp.example.test/auth?redirect_uri={redirect_uri}&state={state}"
client_id = "app-id"
redirect_uri = "http://127.0.0.1:${port}/cb"
capture = { at = "request", from = "query", param = "access_token" }
timeout = "${timeout}"
`);
  return port;
};

const until = async (check) => {
  for (let i = 0; i < 200; i++) {
    const value = check();
    if (value) return value;
    await new Promise((r) => setTimeout(r, 25));
  }
  throw new Error('timed out');
};

const callback = (authorize) => {
  const u = new URL(authorize);
  const back = new URL(u.searchParams.get('redirect_uri'));
  back.searchParams.set('access_token', 'lb-FAKE-0123456789');
  back.searchParams.set('state', u.searchParams.get('state'));
  return new Promise((resolve) => {
    const req = request(back, (res) => {
      res.resume();
      res.on('end', resolve);
    });
    req.on('error', resolve);
    req.end();
  });
};

test('loopback create signs in with --cdp and says so', async () => {
  const sb = sandbox();
  const port = await loopbackProvider(sb);
  const { a } = await browsers('a');
  sb.script('open', 'process.exit(1);');
  const env = { SLICC_JERK_STORE: 'file' };
  const running = sb.jerkAsync(['lb', 'create', '--cdp', String(a.port)], env);
  const authorize = await until(() =>
    a.state.navigations.find((u) => u.startsWith('https://idp.'))
  );
  await callback(authorize);
  const created = await running;
  assert.equal(created.status, 0, created.stderr);
  assert.match(
    created.stderr,
    new RegExp(
      `jerk: opened the sign-in page in the CDP browser http://localhost:${a.port}/json/version \\(via --cdp\\)\\n`
    )
  );
  assert.match(
    created.stderr,
    new RegExp(`waiting up to 10s for the sign-in to redirect to http://127.0.0.1:${port}/cb\\n`)
  );
  assert.ok(created.stderr.startsWith(via(`http://localhost:${a.port}/json/version`, '--cdp')));
  assert.doesNotMatch(created.stdout + created.stderr, /lb-FAKE/);
  assert.deepEqual(
    sb.calls().filter((c) => c.name === 'open'),
    []
  );
});

test('loopback create off a terminal signs in with the browser it finds', async () => {
  const sb = sandbox();
  await loopbackProvider(sb);
  const { seven } = await browsers('seven');
  slicc(sb, [entry(seven, { tier: 1, sevenOpen: true })]);
  const env = { SLICC_JERK_STORE: 'file' };
  const running = sb.jerkAsync(['lb', 'create'], env);
  const authorize = await until(() =>
    seven.state.navigations.find((u) => u.startsWith('https://idp.'))
  );
  await callback(authorize);
  const created = await running;
  assert.equal(created.status, 0, created.stderr);
  assert.match(
    created.stderr,
    /opened the sign-in page in the CDP browser http:\/\/127\.0\.0\.1:\d+\/json\/version \(via slicc browsers\)\n/
  );
});

test('loopback create --cdp auto skips SLICC_CDP_URL', async () => {
  const sb = sandbox();
  await loopbackProvider(sb);
  const b = await browsers('env', 'probe');
  const env = {
    SLICC_JERK_STORE: 'file',
    SLICC_CDP_URL: String(b.env.port),
    SLICC_CDP_PORTS: String(b.probe.port),
  };
  const running = sb.jerkAsync(['lb', 'create', '--cdp', 'auto'], env);
  const authorize = await until(() =>
    b.probe.state.navigations.find((u) => u.startsWith('https://idp.'))
  );
  await callback(authorize);
  const created = await running;
  assert.equal(created.status, 0, created.stderr);
  assert.deepEqual(b.env.state.navigations, []);
  assert.ok(created.stderr.startsWith(via(version(b.probe), 'port probe')), created.stderr);
});

test('loopback create with nothing found says where the page went', async () => {
  const sb = sandbox();
  await loopbackProvider(sb);
  sb.script(
    'open',
    `require('node:fs').writeFileSync(${JSON.stringify(join(sb.dir, 'opened'))}, args[0]);`
  );
  sb.script(
    'xdg-open',
    `require('node:fs').writeFileSync(${JSON.stringify(join(sb.dir, 'opened'))}, args[0]);`
  );
  const env = { SLICC_JERK_STORE: 'file' };
  const running = sb.jerkAsync(['lb', 'create'], env);
  const authorize = await until(() => {
    try {
      return readFileSync(join(sb.dir, 'opened'), 'utf8');
    } catch {
      return null;
    }
  });
  await callback(authorize);
  const created = await running;
  assert.equal(created.status, 0, created.stderr);
  assert.match(created.stderr, /jerk: lb: no CDP browser found\. Tried, in order:\n/);
  assert.match(
    created.stderr,
    /jerk: opened the sign-in page in the system browser \((open|xdg-open)\)\n/
  );
});

test('loopback create refuses a bad --cdp and explains a remote one', async () => {
  const sb = sandbox();
  await loopbackProvider(sb, '1s');
  let res = await sb.jerkAsync(['lb', 'create', '--cdp', '0']);
  assert.equal(res.status, 2);
  assert.equal(res.stderr, `jerk: lb: ${unsupported('0')}\n`);

  sb.script('open', 'process.exit(1);');
  sb.script('xdg-open', 'process.exit(1);');
  const port = await freePort();
  res = await sb.jerkAsync(
    ['lb', 'create', '--cdp', `ws://remote.invalid:${port}/devtools/browser/x`],
    {
      SLICC_JERK_STORE: 'file',
    }
  );
  assert.equal(res.status, 4, res.stderr);
  assert.match(res.stderr, /callback via CDP: remote browser/);
  assert.doesNotMatch(res.stderr, /system browser|open it yourself|waiting up to/);
});

for (const mode of ['auto', 'loopback']) {
  test(`native ${mode} captures before a free callback port on a SLICC relay`, async () => {
    const sb = sandbox();
    const port = await loopbackProvider(sb, '1s', mode);
    const redirect = `http://127.0.0.1:${port}/cb`;
    let sent;
    const cdp = await stubCdp({
      paused: (authorize) => {
        const auth = new URL(authorize);
        sent = auth.searchParams.get('redirect_uri');
        return [
          {
            request: {
              url: sent + '?access_token=FAKE-relay&state=' + auth.searchParams.get('state'),
              method: 'GET',
              headers: {},
            },
          },
        ];
      },
    });
    after(() => cdp.close());
    sb.script(
      'slicc',
      `if (args.join(' ') === 'tray --json') console.log(JSON.stringify([{mode:'cdp', state:'connected', cdp:{port:${new URL(cdp.wsUrl).port}}}]));`
    );
    const res = await sb.jerkAsync(['lb', 'create'], {
      SLICC_JERK_STORE: 'file',
      SLICC_CDP_URL: cdp.wsUrl,
    });
    assert.equal(res.status, 0, res.stderr);
    assert.equal(sent, redirect);
    assert.match(res.stderr, /callback via CDP: browser behind a SLICC relay/);
    assert.doesNotMatch(res.stdout + res.stderr, /FAKE-relay|waiting up to/);
    const commands = cdp
      .log()
      .split('\n')
      .filter((line) => line.startsWith('>> '))
      .map((line) => JSON.parse(line.slice(3)));
    assert.ok(
      commands.findIndex((m) => m.method === 'Fetch.enable') <
        commands.findIndex((m) => m.method === 'Page.navigate')
    );
    assert.deepEqual(
      commands.filter((m) => m.method === 'Target.closeTarget').map((m) => m.params.targetId),
      ['T1']
    );
  });
}
