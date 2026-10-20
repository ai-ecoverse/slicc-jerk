import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const BASE = 'openid https://www.googleapis.com/auth/userinfo.email';
const CALENDAR = 'https://www.googleapis.com/auth/calendar';
const DRIVE = 'https://www.googleapis.com/auth/drive';
const loopback = { skip: process.platform === 'win32' && 'the system browser is rundll32' };

function idp() {
  const posts = [];
  let granted = '';
  let minted = 0;
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (chunk) => {
      body += chunk;
    });
    req.on('end', () => {
      const form = new URLSearchParams(body);
      posts.push({ url: req.url, form });
      res.writeHead(200, { 'content-type': 'application/json' });
      if (req.url === '/revoke') {
        res.end('{}');
        return;
      }
      minted += 1;
      if (form.get('grant_type') === 'authorization_code') granted = form.get('code');
      res.end(
        JSON.stringify({
          access_token: `ya29.FAKE-${minted}-0123456789`,
          token_type: 'Bearer',
          refresh_token: `rt-FAKE-${minted}`,
          expires_in: 3600,
          scope: granted,
        })
      );
    });
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      resolve({
        port: server.address().port,
        posts,
        close: () =>
          new Promise((done) => {
            server.close(done);
            server.closeAllConnections();
          }),
      });
    });
  });
}

function freePort() {
  return new Promise((resolve) => {
    const s = createServer();
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

async function google() {
  const sb = sandbox();
  const server = await idp();
  const port = await freePort();
  const shown = sb.jerk(['config', 'show', 'google']).stdout;
  const stub = `http://127.0.0.1:${server.port}`;
  sb.writeUser(
    shown
      .replaceAll('https://accounts.google.com', stub)
      .replaceAll('https://oauth2.googleapis.com', stub)
      .replace('http://127.0.0.1:56121', `http://127.0.0.1:${port}`)
  );
  const opened = join(sb.dir, 'opened.txt');
  const browser = `const u = new URL(args[0]);
fs.appendFileSync(${JSON.stringify(opened)}, args[0] + '\\n');
const back = new URL(u.searchParams.get('redirect_uri'));
back.searchParams.set('code', u.searchParams.get('scope'));
back.searchParams.set('state', u.searchParams.get('state'));
require('node:child_process').spawn(process.execPath, ['-e', 'fetch(process.argv[1]).catch(() => {})', back.href], { detached: true, stdio: 'ignore' }).unref();`;
  sb.script('open', browser);
  sb.script('xdg-open', browser);
  const env = { SLICC_JERK_STORE: 'file' };
  return {
    sb,
    server,
    run: (args) => sb.jerkAsync(['google', ...args], env),
    authorized: () =>
      readFileSync(opened, 'utf8')
        .trim()
        .split('\n')
        .map((line) => new URL(line).searchParams),
  };
}

test('google is a built-in oauth provider separate from gmail', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'google']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /GOOGLE_ACCESS_TOKEN/);
  assert.match(shown.stdout, /include_granted_scopes=true/);
  assert.match(shown.stdout, /access_type=offline/);
  assert.match(shown.stdout, /GWS_CLIENT_ID/);
  assert.match(shown.stdout, /oauth2\.googleapis\.com\/revoke/);
  assert.match(
    shown.stdout,
    /scope = "openid https:\/\/www\.googleapis\.com\/auth\/userinfo\.email"/
  );
  for (const host of ['www', 'drive', 'sheets', 'docs']) {
    assert.match(shown.stdout, new RegExp(`"${host}\\.googleapis\\.com"`));
  }
  const gmail = sb.jerk(['config', 'show', 'gmail']).stdout;
  assert.match(gmail, /GWS_ACCESS_TOKEN/);
  assert.doesNotMatch(gmail, /include_granted_scopes/);
  const missing = sb.jerk(['google', 'get']);
  assert.equal(missing.status, 3);
  assert.match(missing.stderr, /jerk google create/);
  const fromEnv = sb.jerk(['google', 'get', '--json'], {
    GOOGLE_ACCESS_TOKEN: 'ya29.envtokenvalue1234567',
  });
  assert.equal(JSON.parse(fromEnv.stdout).source, 'env');
});

test('create widens one google grant to the union of its scopes', loopback, async () => {
  const g = await google();
  try {
    const first = await g.run(['create', '--scope', CALENDAR]);
    assert.equal(first.status, 0, first.stderr);
    assert.equal(first.stdout, 'google: signed in\n');

    const covered = await g.run(['get', '--scope', CALENDAR, '--json']);
    assert.equal(covered.status, 0, covered.stderr);
    assert.equal(JSON.parse(covered.stdout).scopes, CALENDAR);

    const uncovered = await g.run(['get', '--scope', DRIVE]);
    assert.equal(uncovered.status, 3);
    assert.match(
      uncovered.stderr,
      /does not cover https:\/\/www\.googleapis\.com\/auth\/drive; run jerk google create --scope/
    );

    const again = await g.run(['create', '--scope', CALENDAR]);
    assert.equal(again.stdout, 'google: already signed in; use --force to sign in again\n');

    const wider = await g.run(['create', '-p', `scope=${DRIVE}`]);
    assert.equal(wider.status, 0, wider.stderr);
    const [one, two] = g.authorized();
    assert.equal(g.authorized().length, 2);
    assert.equal(one.get('scope'), CALENDAR);
    assert.equal(two.get('scope'), `${CALENDAR} ${DRIVE}`);
    assert.equal(two.get('include_granted_scopes'), 'true');
    assert.equal(two.get('access_type'), 'offline');

    const both = await g.run(['get', '--scope', `${DRIVE} ${CALENDAR}`, '--json', '--reveal']);
    assert.equal(both.status, 0, both.stderr);
    const doc = JSON.parse(both.stdout);
    assert.equal(doc.scopes, `${CALENDAR} ${DRIVE}`);
    assert.match(doc.token, /^ya29\.FAKE-/);
    const refreshes = g.server.posts.filter((p) => p.form.get('grant_type') === 'refresh_token');
    assert.ok(refreshes.length >= 1);
    assert.match(refreshes.at(-1).form.get('refresh_token'), /^rt-FAKE-/);

    const dropped = await g.run(['drop', '--yes']);
    assert.equal(dropped.status, 0, dropped.stderr);
    assert.equal(g.server.posts.filter((p) => p.url === '/revoke').length, 1);
    const gone = await g.run(['get']);
    assert.equal(gone.status, 3);
  } finally {
    await g.server.close();
  }
});

test('a bare google create asks only for the default scope', loopback, async () => {
  const g = await google();
  try {
    const first = await g.run(['create']);
    assert.equal(first.status, 0, first.stderr);
    assert.equal(g.authorized()[0].get('scope'), BASE);
    const again = await g.run(['create']);
    assert.equal(again.stdout, 'google: already signed in; use --force to sign in again\n');
    const forced = await g.run(['create', '--force', '--scope', DRIVE]);
    assert.equal(forced.status, 0, forced.stderr);
    assert.equal(g.authorized()[1].get('scope'), DRIVE);
  } finally {
    await g.server.close();
  }
});
