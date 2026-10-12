import assert from 'node:assert/strict';
import { test } from 'node:test';
import { expectedVersion, sandbox } from './helpers.mjs';

test('--version prints the version', () => {
  const sb = sandbox();
  for (const flag of ['--version', '-V']) {
    const res = sb.jerk([flag]);
    assert.equal(res.status, 0);
    assert.equal(res.stdout, `jerk ${expectedVersion}\n`);
    assert.equal(res.stderr, '');
  }
});

test('--help and no arguments print usage', () => {
  const sb = sandbox();
  for (const args of [[], ['--help'], ['-h']]) {
    const res = sb.jerk(args);
    assert.equal(res.status, 0);
    assert.match(res.stdout, /^jerk: get, list, create and drop OAuth tokens/);
    assert.match(res.stdout, /--config <file>/);
    assert.match(
      res.stdout,
      /Exit codes: 0 ok, 1 error, 2 usage, 3 sign-in needed, 4 not available here, 5 unsupported/
    );
  }
});

test('unknown options, commands and providers exit 2', () => {
  const sb = sandbox();
  assert.equal(sb.jerk(['--nope']).status, 2);
  assert.match(sb.jerk(['github', 'steal']).stderr, /unknown command steal/);
  assert.equal(sb.jerk(['github']).status, 2);
  const unknown = sb.jerk(['nope', 'get']);
  assert.equal(unknown.status, 2);
  assert.match(unknown.stderr, /unknown provider nope/);
});

test('jerk list shows the built-in providers and finds their CLIs on PATH', () => {
  const sb = sandbox();
  sb.fakeCli('gh');
  const res = sb.jerk(['list']);
  assert.equal(res.status, 0, res.stderr);
  const lines = res.stdout.trimEnd().split('\n');
  assert.match(lines[0], /^PROVIDER\s+NAME\s+SOURCE\s+CLI$/);
  assert.ok(
    lines.some((l) => /^github\s+GitHub \(gh\)\s+built-in\s+gh \(found\)$/.test(l)),
    res.stdout
  );
  assert.ok(
    lines.some((l) => /^npm\s+npm registry\s+built-in\s+npm \(missing\)$/.test(l)),
    res.stdout
  );
  const json = JSON.parse(sb.jerk(['list', '--json']).stdout);
  assert.deepEqual(
    json.map((p) => p.provider),
    ['adobe-aio', 'cloudflare', 'github', 'npm']
  );
  assert.equal(json.find((p) => p.provider === 'github').cli, true);
});

test('a user file replaces a built-in provider as a unit', () => {
  const sb = sandbox();
  sb.writeUser('[providers.github]\nenv = ["MY_GH"]\noauth = { mode = "device" }\n');
  const json = JSON.parse(sb.jerk(['list', '--json']).stdout);
  const github = json.find((p) => p.provider === 'github');
  assert.equal(github.source, 'user');
  assert.deepEqual(github.env, ['MY_GH']);
  assert.equal(github.requires, null);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 0);
  assert.equal(check.stdout, 'ok: 4 providers (3 built-in, 1 user)\n');
});

test('config check reports every problem and exits 2', () => {
  const sb = sandbox();
  sb.writeUser('[providers.Bad]\nenv = ["1A"]\ndomains = ["*.x.com"]\n');
  const res = sb.jerk(['config', 'check']);
  assert.equal(res.status, 2);
  assert.match(res.stderr, /Bad: the id must be lower-case/);
  assert.match(res.stderr, /env name "1A"/);
  assert.match(res.stderr, /domain "\*\.x\.com" must be an exact host name/);
  assert.equal(sb.jerk(['list']).status, 2);
});

test('--config adds an explicit layer, and a missing one is an error', () => {
  const sb = sandbox();
  const missing = sb.jerk(['--config', `${sb.dir}/nope.toml`, 'config', 'check']);
  assert.equal(missing.status, 2);
  assert.match(missing.stderr, /cannot read/);
  const path = sb.jerk(['--config', `${sb.dir}/nope.toml`, 'config', 'path', '--json']);
  assert.equal(path.status, 0);
  assert.equal(JSON.parse(path.stdout).explicit.exists, false);
});

test('config show prints the effective provider as TOML', () => {
  const sb = sandbox();
  const res = sb.jerk(['config', 'show', 'github']);
  assert.equal(res.status, 0);
  assert.match(res.stdout, /^\[providers\.github\]/m);
  assert.match(res.stdout, /run = \["gh", "auth", "token", "--hostname", "\{host\}"\]/);
  assert.equal(res.stderr, 'jerk: github comes from built-in\n');
});

test('provider commands exit 5 until they are built', () => {
  const sb = sandbox();
  for (const args of [['npm', 'lease'], ['github', 'sync'], ['config', 'fnox'], ['intercept']]) {
    const res = sb.jerk(args);
    assert.equal(res.status, 5, args.join(' '));
    assert.match(res.stderr, /not implemented yet/);
    assert.equal(res.stdout, '');
  }
});
