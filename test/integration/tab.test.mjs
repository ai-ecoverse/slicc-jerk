import assert from 'node:assert/strict';
import { test } from 'node:test';
import { stubBrowser } from './cdp-stub.mjs';
import { sandbox } from './helpers.mjs';

const app = 'https://app.example.test/inbox?folder=1';
const appOrigin = 'https://app.example.test';
const signedIn = () => ({
  pages: [
    { targetId: 'AAAA1111', url: 'https://other.example.test/', title: 'Other' },
    { targetId: 'BBBB2222', url: app, title: 'Inbox' },
  ],
  cookies: [
    {
      name: 'sid',
      value: 'fake-session',
      domain: 'app.example.test',
      path: '/',
      expires: 2000000000,
      size: 16,
      httpOnly: true,
      secure: true,
      session: false,
      sameSite: 'Lax',
    },
    {
      name: 'pref',
      value: 'dark',
      domain: '.example.test',
      path: '/',
      expires: -1,
      size: 8,
      httpOnly: false,
      secure: false,
      session: true,
    },
  ],
  storage: {
    [appOrigin]: {
      localStorage: { auth: 'fake-local-token' },
      sessionStorage: { step: '2' },
    },
  },
});

const persistent = (browser) =>
  browser.state.calls.filter((m) => /^Page\.addScriptToEvaluateOn|^Runtime\.addBinding/.test(m));

test('tab export and import carry cookies and storage into a new tab in another browser', async () => {
  const sb = sandbox();
  const from = await stubBrowser(signedIn());
  const to = await stubBrowser({});
  try {
    const listed = await sb.jerkAsync(['tab', 'list', '--json', '--cdp', String(from.port)]);
    assert.equal(listed.status, 0, listed.stderr);
    assert.deepEqual(
      JSON.parse(listed.stdout).map((p) => p.id),
      ['AAAA1111', 'BBBB2222']
    );

    const exported = await sb.jerkAsync([
      'tab',
      'export',
      '--url',
      'app.example.test/inbox',
      '--cdp',
      String(from.port),
    ]);
    assert.equal(exported.status, 0, exported.stderr);
    assert.match(exported.stderr, /exported 2 cookies and 2 storage entries from https:\/\/app/);
    assert.match(exported.stdout, /^[A-Za-z0-9+/=]+\n$/);
    assert.doesNotMatch(exported.stdout, /fake-session/);

    const imported = await sb.jerkAsync(['tab', 'import', '--cdp', to.wsUrl], {}, exported.stdout);
    assert.equal(imported.status, 0, imported.stderr);
    assert.equal(imported.stdout, 'NEW1\n');
    assert.match(imported.stderr, /imported 2 cookies and 2 storage entries into tab NEW1/);
    assert.equal(to.state.pages.find((p) => p.targetId === 'NEW1').url, app);
    assert.deepEqual(to.state.storage[appOrigin], {
      localStorage: { auth: 'fake-local-token' },
      sessionStorage: { step: '2' },
    });
    assert.deepEqual(to.state.setCookies, [
      {
        name: 'sid',
        value: 'fake-session',
        path: '/',
        secure: true,
        httpOnly: true,
        url: 'https://app.example.test/',
        expires: 2000000000,
        sameSite: 'Lax',
      },
      {
        name: 'pref',
        value: 'dark',
        path: '/',
        secure: false,
        httpOnly: false,
        domain: '.example.test',
      },
    ]);
    assert.deepEqual(to.state.navigations, [`${appOrigin}/robots.txt`, app]);
    assert.deepEqual(persistent(to), []);
  } finally {
    await from.close();
    await to.close();
  }
});

test('tab import into an existing tab picked by id prefix', async () => {
  const sb = sandbox();
  const from = await stubBrowser(signedIn());
  const to = await stubBrowser({ pages: [{ targetId: 'CCCC3333', url: 'about:blank' }] });
  try {
    const exported = await sb.jerkAsync(['tab', 'export', '--tab', 'bbbb', '--cdp', from.wsUrl]);
    assert.equal(exported.status, 0, exported.stderr);
    const imported = await sb.jerkAsync(
      ['tab', 'import', '--tab', 'cccc', '--cdp', `http://127.0.0.1:${to.port}`],
      {},
      exported.stdout
    );
    assert.equal(imported.status, 0, imported.stderr);
    assert.equal(imported.stdout, 'CCCC3333\n');
    assert.equal(to.state.pages[0].url, app);
  } finally {
    await from.close();
    await to.close();
  }
});

test('tab import fails when the tab lands on another origin before storage is written', async () => {
  const sb = sandbox();
  const from = await stubBrowser(signedIn());
  const to = await stubBrowser({});
  to.state.redirect = 'https://login.example.test/sso';
  try {
    const exported = await sb.jerkAsync(['tab', 'export', '--url', 'inbox', '--cdp', from.wsUrl]);
    const imported = await sb.jerkAsync(['tab', 'import', '--cdp', to.wsUrl], {}, exported.stdout);
    assert.equal(imported.status, 1, imported.stderr);
    assert.match(
      imported.stderr,
      /ended up on https:\/\/login\.example\.test\/sso, so the storage for https:\/\/app\.example\.test was not written/
    );
    assert.equal(to.state.storage[appOrigin], undefined);
    assert.deepEqual(to.state.navigations, [`${appOrigin}/robots.txt`, `${appOrigin}/`]);
    assert.deepEqual(persistent(to), []);
  } finally {
    await from.close();
    await to.close();
  }
});

test('tab import keeps the storage it wrote when the bundle URL then lands on another origin', async () => {
  const sb = sandbox();
  const from = await stubBrowser(signedIn());
  const to = await stubBrowser({});
  to.state.redirects[app] = 'https://login.example.test/sso';
  try {
    const exported = await sb.jerkAsync(['tab', 'export', '--url', 'inbox', '--cdp', from.wsUrl]);
    const imported = await sb.jerkAsync(['tab', 'import', '--cdp', to.wsUrl], {}, exported.stdout);
    assert.equal(imported.status, 0, imported.stderr);
    assert.match(imported.stderr, /the tab ended up on https:\/\/login\.example\.test\/sso\n$/);
    assert.deepEqual(to.state.storage[appOrigin], {
      localStorage: { auth: 'fake-local-token' },
      sessionStorage: { step: '2' },
    });
    assert.deepEqual(to.state.navigations, [`${appOrigin}/robots.txt`, app]);
  } finally {
    await from.close();
    await to.close();
  }
});

test('tab import writes the storage before the origin root can redirect a signed-out visitor', async () => {
  const sb = sandbox();
  const from = await stubBrowser(signedIn());
  const to = await stubBrowser({});
  to.state.scripted[`${appOrigin}/`] = 'https://login.example.test/sso';
  try {
    const exported = await sb.jerkAsync(['tab', 'export', '--url', 'inbox', '--cdp', from.wsUrl]);
    const imported = await sb.jerkAsync(['tab', 'import', '--cdp', to.wsUrl], {}, exported.stdout);
    assert.equal(imported.status, 0, imported.stderr);
    assert.doesNotMatch(imported.stderr, /ended up on/);
    assert.deepEqual(to.state.storage[appOrigin], {
      localStorage: { auth: 'fake-local-token' },
      sessionStorage: { step: '2' },
    });
    assert.deepEqual(to.state.navigations, [`${appOrigin}/robots.txt`, app]);
    assert.deepEqual(persistent(to), []);
  } finally {
    await from.close();
    await to.close();
  }
});

test('tab import falls back to the origin root when robots.txt is an error page', async () => {
  const sb = sandbox();
  const from = await stubBrowser(signedIn());
  const to = await stubBrowser({});
  to.state.redirects[`${appOrigin}/robots.txt`] = 'chrome-error://chromewebdata/';
  try {
    const exported = await sb.jerkAsync(['tab', 'export', '--url', 'inbox', '--cdp', from.wsUrl]);
    const imported = await sb.jerkAsync(['tab', 'import', '--cdp', to.wsUrl], {}, exported.stdout);
    assert.equal(imported.status, 0, imported.stderr);
    assert.deepEqual(to.state.storage[appOrigin], {
      localStorage: { auth: 'fake-local-token' },
      sessionStorage: { step: '2' },
    });
    assert.deepEqual(to.state.storage.null, { localStorage: {}, sessionStorage: {} });
    assert.deepEqual(to.state.navigations, [`${appOrigin}/robots.txt`, `${appOrigin}/`, app]);
  } finally {
    await from.close();
    await to.close();
  }
});

test('tab export refuses ambiguous or missing tabs, and import refuses a non-bundle', async () => {
  const sb = sandbox();
  const from = await stubBrowser(signedIn());
  try {
    const many = await sb.jerkAsync([
      'tab',
      'export',
      '--url',
      'example.test',
      '--cdp',
      from.wsUrl,
    ]);
    assert.equal(many.status, 2);
    assert.match(many.stderr, /2 tabs match --url example\.test/);
    const none = await sb.jerkAsync(['tab', 'export', '--tab', 'ffff', '--cdp', from.wsUrl]);
    assert.equal(none.status, 1);
    assert.match(none.stderr, /no tab matches --tab ffff/);
    const bare = await sb.jerkAsync(['tab', 'export', '--cdp', from.wsUrl]);
    assert.equal(bare.status, 2);
    assert.match(bare.stderr, /export needs --tab/);
    const junk = await sb.jerkAsync(['tab', 'import', '--cdp', from.wsUrl], {}, 'not base64!\n');
    assert.equal(junk.status, 2);
    assert.match(junk.stderr, /not a jerk tab bundle/);
    const script = Buffer.from(
      JSON.stringify({ kind: 'jerk-tab', v: 1, url: app, origin: 'javascript:alert(1)' })
    ).toString('base64');
    const foreign = await sb.jerkAsync(['tab', 'import', '--cdp', from.wsUrl], {}, `${script}\n`);
    assert.equal(foreign.status, 2);
    assert.match(foreign.stderr, /not a jerk tab bundle/);
    assert.equal(from.state.calls.includes('Page.navigate'), false);
  } finally {
    await from.close();
  }
});
