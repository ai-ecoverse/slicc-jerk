import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

test('linkedin is a built-in oauth provider that needs app credentials', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'linkedin']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /flow = "code"/);
  assert.match(shown.stdout, /mode = "intercept"/);
  assert.match(shown.stdout, /www\.linkedin\.com\/oauth\/v2\/authorization/);
  assert.match(shown.stdout, /oauth\/v2\/accessToken/);
  assert.match(shown.stdout, /LINKEDIN_ACCESS_TOKEN/);
  assert.match(shown.stdout, /client_id = "\{client_id\}"/);
  assert.match(shown.stdout, /client_secret = "\{client_secret\}"/);
  assert.match(shown.stdout, /redirect_uri = "http:\/\/localhost\/callback"/);

  const missing = sb.jerk(['linkedin', 'create']);
  assert.equal(missing.status, 2);
  assert.match(missing.stderr, /-p client_id=/);

  const secret = sb.jerk(['linkedin', 'create', '-p', 'client_id=app-id']);
  assert.equal(secret.status, 2);
  assert.match(secret.stderr, /-p client_secret=/);
});

test('linkedin get uses LINKEDIN_ACCESS_TOKEN from the environment', () => {
  const sb = sandbox();
  const masked = sb.jerk(['linkedin', 'get'], { LINKEDIN_ACCESS_TOKEN: 'li-env-token-1234567890' });
  assert.equal(masked.status, 0, masked.stderr);
  assert.equal(masked.stdout, 'li-e…7890\n');
  const revealed = sb.jerk(['linkedin', 'get', '--reveal'], {
    LINKEDIN_ACCESS_TOKEN: 'li-env-token-1234567890',
  });
  assert.equal(revealed.status, 0, revealed.stderr);
  assert.equal(revealed.stdout, 'li-env-token-1234567890\n');
});

test('linkedin create with credentials needs a CDP browser', () => {
  const sb = sandbox();
  const res = sb.jerk([
    'linkedin',
    'create',
    '-p',
    'client_id=fake-app-id',
    '-p',
    'client_secret=fake-app-secret',
  ]);
  assert.equal(res.status, 4, res.stderr);
  assert.match(res.stderr, /CDP|cdp|slicc launch/i);
});
