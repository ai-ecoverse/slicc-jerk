import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const KEY =
  'eyJhbGciOiJub25lIn0.eyJqdGkiOiJ0ZXN0LWp0aS0xIiwiZXhwIjo5OTk5OTk5OTk5LCJzdWIiOiJvcmcvc2l0ZSJ9.';
const JTI = 'test-jti-1';
const LEAK = 'LEAKY-AEM-OUTPUT';

function aemExt(sb, { create = true, keys = [{ id: JTI, description: 'jerk', roles: ['admin'] }] } = {}) {
  sb.script(
    'aem-ext',
    `if (args[0] === 'auth' && args[1] === 'key' && args[2] === 'create') {
  if (!args.includes('--json') || !args.includes('--register')) { process.stderr.write('bad create flags\\n'); process.exit(1); }
  const org = args[args.indexOf('--org') + 1];
  const site = args[args.indexOf('--site') + 1];
  if (!org || !site) { process.stderr.write('missing org/site\\n'); process.exit(1); }
  process.stdout.write(JSON.stringify({
    id: ${JSON.stringify(JTI)},
    subject: org + '/' + site,
    description: 'jerk',
    roles: ['admin'],
    value: ${JSON.stringify(create ? KEY : '')},
    registered: true,
  }));
  process.exit(${create ? 0 : 1});
}
if (args[0] === 'auth' && args[1] === 'key' && args[2] === 'list') {
  process.stdout.write(JSON.stringify({
    org: args[args.indexOf('--org') + 1],
    site: args[args.indexOf('--site') + 1],
    registeredIds: [${JSON.stringify(JTI)}],
    keys: ${JSON.stringify(keys)},
  }));
  process.exit(0);
}
if (args[0] === 'auth' && args[1] === 'key' && args[2] === 'delete') {
  if (!args.includes('--confirm')) { process.stderr.write('need --confirm\\n'); process.exit(1); }
  process.exit(0);
}
process.stdout.write(${JSON.stringify(`${LEAK}\n`)});
process.exit(1);`
  );
}

test('without aem-ext, create still needs org and site for oauth', () => {
  const sb = sandbox();
  const missing = sb.jerk(['aem', 'create']);
  assert.equal(missing.status, 2);
  assert.match(missing.stderr, /-p org=/);
  assert.doesNotMatch(missing.stdout + missing.stderr, new RegExp(LEAK));
});

test('with aem-ext, create mints a key, list shows it, drop revokes by id', () => {
  const sb = sandbox();
  aemExt(sb);
  const created = sb.jerk([
    'aem',
    'create',
    '--reveal',
    '-p',
    'org=acme',
    '-p',
    'site=www',
  ]);
  assert.equal(created.status, 0, created.stderr);
  assert.equal(created.stdout, `${KEY}\n`);
  assert.match(created.stderr, /created token test-jti-1/);
  assert.doesNotMatch(created.stdout + created.stderr, new RegExp(LEAK));

  const listed = sb.jerk(['aem', 'list', '--json', '-p', 'org=acme', '-p', 'site=www']);
  assert.equal(listed.status, 0, listed.stderr);
  assert.deepEqual(JSON.parse(listed.stdout), {
    items: [{ id: JTI, description: 'jerk', roles: ['admin'] }],
  });

  const dropped = sb.jerk([
    'aem',
    'drop',
    JTI,
    '--yes',
    '-p',
    'org=acme',
    '-p',
    'site=www',
  ]);
  assert.equal(dropped.status, 0, dropped.stderr);
  assert.equal(dropped.stdout, 'aem: dropped\n');

  const runs = sb
    .calls()
    .filter((c) => c.name === 'aem-ext')
    .map((c) => c.args.slice(0, 3));
  assert.deepEqual(runs, [
    ['auth', 'key', 'create'],
    ['auth', 'key', 'list'],
    ['auth', 'key', 'delete'],
  ]);
  const del = sb.calls().find((c) => c.args[2] === 'delete');
  assert.ok(del.args.includes('--confirm'));
  assert.ok(del.args.includes(JTI));
});

test('env AEM_TOKEN wins for get even when aem-ext is present', () => {
  const sb = sandbox();
  aemExt(sb);
  const res = sb.jerk(['aem', 'get', '--json'], { AEM_TOKEN: 'aemenvtokenvalue1234567' });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(JSON.parse(res.stdout).source, 'env');
  assert.equal(sb.calls().length, 0);
});

test('list --json shows aem-ext then oauth in the chain', () => {
  const sb = sandbox();
  const entry = JSON.parse(sb.jerk(['list', '--json']).stdout).find((p) => p.provider === 'aem');
  assert.deepEqual(entry.steps, [
    { cli: 'aem-ext', found: false },
    { oauth: true },
  ]);
  assert.deepEqual(entry.env, ['AEM_TOKEN', 'AEM_API_KEY']);
  const listed = sb.jerk(['list']);
  assert.match(listed.stdout, /\naem\s.*\saem-ext \(missing\), oauth\n/);
});

test('config show aem keeps oauth capture and shows the aem-ext create step', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'aem']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /\[\[providers\.aem\.create\]\]/);
  assert.match(shown.stdout, /requires = "aem-ext"/);
  assert.match(shown.stdout, /from = "body"/);
  assert.match(shown.stdout, /param = "authToken"/);
});
