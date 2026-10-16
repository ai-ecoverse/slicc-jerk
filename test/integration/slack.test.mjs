import assert from 'node:assert/strict';
import { test } from 'node:test';
import { stubCdp } from './cdp-stub.mjs';
import { sandbox } from './helpers.mjs';

const token = 'xoxc-FAKESESSIONTOKEN1234';
const cookie = 'xoxd-FAKECOOKIEVALUE9876';
const otherToken = 'xoxc-OTHERSESSIONTOKEN5678';
const otherCookie = 'xoxd-OTHERCOOKIEVALUE5432';

function apiPost(workspaceToken, workspaceCookie, status) {
  const params = {
    resourceType: 'XHR',
    request: {
      url: 'https://app.slack.com/api/client.boot',
      method: 'POST',
      headers: {
        Cookie: `d=${workspaceCookie}; shown=1`,
        'Content-Type': 'application/x-www-form-urlencoded',
      },
      postData: `token=${workspaceToken}&version=1`,
    },
  };
  if (status != null) {
    params.responseStatusCode = status;
    params.responseStatusText = 'OK';
  }
  return params;
}

test('slack is a built-in session capture provider', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'slack']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /SLACK_TOKEN/);
  assert.match(shown.stdout, /SLACK_D_COOKIE/);
  assert.match(shown.stdout, /mode = "intercept"/);
  assert.match(shown.stdout, /stage = "request"/);
  assert.match(shown.stdout, /cookie:d/);
  assert.match(shown.stdout, /app\.slack\.com\/client\/\{workspace\}/);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 0, check.stderr);
  const listed = sb.jerk(['slack', 'list']);
  assert.equal(listed.status, 5);
  assert.match(listed.stderr, /has no list command/);
});

test('slack get says not signed in without a grant or env token', () => {
  const sb = sandbox();
  const res = sb.jerk(['slack', 'get', '-p', 'workspace=T123']);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /sign in with jerk slack create/);
});

test('slack get prefers SLACK_TOKEN and keeps a paired cookie', () => {
  const sb = sandbox();
  const both = sb.jerk(['slack', 'get', '-p', 'workspace=T123'], {
    SLACK_TOKEN: token,
    SLACK_D_COOKIE: cookie,
  });
  assert.equal(both.status, 0, both.stderr);
  assert.equal(both.stdout, `SLACK_D_COOKIE=xoxd…9876\nSLACK_TOKEN=xoxc…1234\n`);
  const revealed = sb.jerk(['slack', 'get', '--reveal', '-p', 'workspace=T123'], {
    SLACK_TOKEN: token,
    SLACK_D_COOKIE: cookie,
  });
  assert.equal(revealed.stdout, `SLACK_D_COOKIE=${cookie}\nSLACK_TOKEN=${token}\n`);
  const only = sb.jerk(['slack', 'get', '--reveal', '-p', 'workspace=T123'], {
    SLACK_TOKEN: token,
  });
  assert.equal(only.status, 0, only.stderr);
  assert.equal(only.stdout.trim(), token);
});

test('slack create captures the token and d cookie for one workspace', async () => {
  const sb = sandbox();
  const missing = sb.jerk(['slack', 'create']);
  assert.equal(missing.status, 2, missing.stderr);
  assert.match(missing.stderr, /workspace/);

  const cdp = await stubCdp({
    paused: [
      apiPost('xoxc-RESPONSESTAGEONLY9999', 'xoxd-RESPONSESTAGEONLY9999', 200),
      apiPost(token, cookie),
    ],
  });
  const other = await stubCdp({ paused: [apiPost(otherToken, otherCookie)] });
  try {
    const created = await sb.jerkAsync(
      ['slack', 'create', '--force', '--cdp', cdp.wsUrl, '-p', 'workspace=T123'],
      { SLICC_JERK_STORE: 'file' }
    );
    assert.equal(created.status, 0, `${created.stderr}\n--- cdp ---\n${cdp.log()}`);
    assert.match(created.stdout, /slack: signed in/);
    assert.doesNotMatch(created.stdout + created.stderr, /xoxc-FAKE|xoxd-FAKE/);
    assert.match(cdp.log(), /https:\/\/app\.slack\.com\/client\/T123/);
    assert.match(cdp.log(), /"requestStage":"Request"/);

    const second = await sb.jerkAsync(
      ['slack', 'create', '--force', '--cdp', other.wsUrl, '-p', 'workspace=E999'],
      { SLICC_JERK_STORE: 'file' }
    );
    assert.equal(second.status, 0, second.stderr);

    const masked = sb.jerk(['slack', 'get', '-p', 'workspace=T123'], { SLICC_JERK_STORE: 'file' });
    assert.equal(masked.status, 0, masked.stderr);
    assert.equal(masked.stdout, `SLACK_D_COOKIE=xoxd…9876\nSLACK_TOKEN=xoxc…1234\n`);
    const revealed = sb.jerk(['slack', 'get', '--reveal', '-p', 'workspace=T123'], {
      SLICC_JERK_STORE: 'file',
    });
    assert.equal(revealed.stdout, `SLACK_D_COOKIE=${cookie}\nSLACK_TOKEN=${token}\n`);
    const elsewhere = sb.jerk(['slack', 'get', '--reveal', '-p', 'workspace=E999'], {
      SLICC_JERK_STORE: 'file',
    });
    assert.equal(elsewhere.stdout, `SLACK_D_COOKIE=${otherCookie}\nSLACK_TOKEN=${otherToken}\n`);

    const dropped = sb.jerk(['slack', 'drop', '--yes', '-p', 'workspace=T123'], {
      SLICC_JERK_STORE: 'file',
    });
    assert.equal(dropped.status, 0, dropped.stderr);
    assert.match(dropped.stdout, /slack: dropped/);
    const gone = sb.jerk(['slack', 'get', '-p', 'workspace=T123'], { SLICC_JERK_STORE: 'file' });
    assert.equal(gone.status, 3);
    const kept = sb.jerk(['slack', 'get', '--reveal', '-p', 'workspace=E999'], {
      SLICC_JERK_STORE: 'file',
    });
    assert.equal(kept.stdout, `SLACK_D_COOKIE=${otherCookie}\nSLACK_TOKEN=${otherToken}\n`);
  } finally {
    await cdp.close();
    await other.close();
  }
});
