import assert from 'node:assert/strict';
import { test } from 'node:test';
import { stubCdp } from './cdp-stub.mjs';
import { sandbox } from './helpers.mjs';

const submit = 'https://www.rescuetime.com/anapi/setup_submit';

function keyForm(candidate, status) {
  return {
    resourceType: 'Document',
    responseStatusCode: status,
    responseStatusText: status < 400 ? 'Found' : 'Unprocessable',
    responseHeaders: [
      { name: 'Location', value: 'https://www.rescuetime.com/rtx/settings/api/key_setup_summary' },
    ],
    request: {
      url: submit,
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      postData: `authenticity_token=csrfFAKE&candidate=${candidate}&label=jerk&net_origin_acl=&button=`,
    },
  };
}

test('rescuetime is a built-in intercept provider that captures the key form', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'rescuetime']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /mode = "intercept"/);
  assert.match(shown.stdout, /at = "network"/);
  assert.match(shown.stdout, /from = "body"/);
  assert.match(shown.stdout, /param = "candidate"/);
  assert.match(shown.stdout, /RESCUETIME_API_KEY/);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 0, check.stderr);
});

test('rescuetime get says not signed in without a grant or env key', () => {
  const sb = sandbox();
  const res = sb.jerk(['rescuetime', 'get']);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /sign in with jerk rescuetime create/);
});

test('rescuetime get prefers RESCUETIME_API_KEY', () => {
  const sb = sandbox();
  const res = sb.jerk(['rescuetime', 'get', '--reveal'], {
    RESCUETIME_API_KEY: 'rtEnvKeyFAKE1234567890',
  });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(res.stdout.trim(), 'rtEnvKeyFAKE1234567890');
});

test('rescuetime create keeps the key from the first successful form post', async () => {
  const sb = sandbox();
  const cdp = await stubCdp({
    paused: [
      keyForm('rtRejectedKeyFAKE1234567890', 422),
      keyForm('rtCreatedKeyFAKE1234567890', 302),
    ],
  });
  try {
    const created = await sb.jerkAsync(['rescuetime', 'create', '--force', '--cdp', cdp.wsUrl], {
      SLICC_JERK_STORE: 'file',
    });
    assert.equal(created.status, 0, `${created.stderr}\n--- cdp ---\n${cdp.log()}`);
    assert.match(created.stdout, /rescuetime: signed in/);
    assert.doesNotMatch(created.stdout + created.stderr, /rtCreatedKeyFAKE/);
    assert.match(
      cdp.log(),
      /"requestStage":"Response","urlPattern":"https:\/\/www\.rescuetime\.com\/anapi\/setup_submit"/
    );

    const got = sb.jerk(['rescuetime', 'get', '--reveal'], { SLICC_JERK_STORE: 'file' });
    assert.equal(got.status, 0, got.stderr);
    assert.equal(got.stdout.trim(), 'rtCreatedKeyFAKE1234567890');
  } finally {
    await cdp.close();
  }
});
