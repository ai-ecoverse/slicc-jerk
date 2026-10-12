import assert from 'node:assert/strict';
import { test } from 'node:test';
import { expectedVersion, jerk } from './helpers.mjs';

test('--version prints the version', () => {
  for (const flag of ['--version', '-V']) {
    const res = jerk([flag]);
    assert.equal(res.status, 0);
    assert.equal(res.stdout, `jerk ${expectedVersion}\n`);
    assert.equal(res.stderr, '');
  }
});

test('--help and no arguments print usage', () => {
  for (const args of [[], ['--help'], ['-h']]) {
    const res = jerk(args);
    assert.equal(res.status, 0);
    assert.match(res.stdout, /^jerk: get, list, create and drop OAuth tokens/);
    assert.match(res.stdout, /jerk <provider> get/);
    assert.match(
      res.stdout,
      /Exit codes: 0 ok, 1 error, 2 usage, 3 sign-in needed, 4 not available here, 5 unsupported/
    );
  }
});

test('unknown options and commands exit 2', () => {
  const option = jerk(['--nope']);
  assert.equal(option.status, 2);
  assert.match(option.stderr, /unknown option --nope/);
  const verb = jerk(['github', 'steal']);
  assert.equal(verb.status, 2);
  assert.match(verb.stderr, /unknown command steal/);
  const bare = jerk(['github']);
  assert.equal(bare.status, 2);
});

test('planned commands exit 5 until they are built', () => {
  for (const args of [['list'], ['github', 'get'], ['npm', 'lease'], ['config'], ['intercept']]) {
    const res = jerk(args);
    assert.equal(res.status, 5, args.join(' '));
    assert.match(res.stderr, /not implemented yet/);
    assert.equal(res.stdout, '');
  }
});
