import assert from 'node:assert/strict';
import { test } from 'node:test';
import { listen, stubCdp } from './cdp-stub.mjs';
import { sandbox } from './helpers.mjs';

test('garmin is a built-in intercept exchange provider', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'garmin']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /mode = "intercept"/);
  assert.match(shown.stdout, /flow = "exchange"/);
  assert.match(shown.stdout, /param = "ticket"/);
  assert.match(shown.stdout, /GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2/);
  assert.match(shown.stdout, /service_ticket = "\{value\}"/);
  assert.match(shown.stdout, /diauth\.garmin\.com/);
  assert.match(shown.stdout, /connectapi\.garmin\.com/);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 0, check.stderr);
});

test('garmin get says not signed in without a grant or env token', () => {
  const sb = sandbox();
  const res = sb.jerk(['garmin', 'get']);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /sign in with jerk garmin create/);
});

test('garmin create needs a CDP browser', () => {
  const sb = sandbox();
  const res = sb.jerk(['garmin', 'create']);
  assert.equal(res.status, 4);
  assert.match(res.stderr, /CDP|cdp|browser/);
});

test('garmin create exchanges a captured ticket through stub CDP and token HTTP', async () => {
  const sb = sandbox();
  const posts = [];
  const http = await listen((req, res) => {
    if (req.method === 'POST' && req.url === '/oauth/token') {
      let body = '';
      req.on('data', (c) => {
        body += c;
      });
      req.on('end', () => {
        posts.push(body);
        const access = body.includes('grant_type=refresh_token')
          ? 'garminRefreshedAccessFAKE1234567890'
          : 'garminAccessTokenFAKE1234567890';
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(
          JSON.stringify({
            access_token: access,
            refresh_token: 'garminRefreshTokenFAKE1234567890',
            token_type: 'Bearer',
            expires_in: 93600,
          })
        );
      });
      return;
    }
    res.writeHead(404);
    res.end();
  });
  const cdp = await stubCdp({
    paused: [
      {
        resourceType: 'Document',
        request: {
          url: 'https://connect.garmin.com/modern?ticket=ST-fake-ticket-123',
          method: 'GET',
          headers: { Accept: 'text/html' },
        },
      },
    ],
  });
  try {
    sb.writeUser(`[providers.garmin]
name = "Garmin Connect"
env = ["GARMIN_TOKEN"]
store = "file"
domains = ["connectapi.garmin.com"]
[providers.garmin.oauth]
mode = "intercept"
flow = "exchange"
authorize_url = "https://sso.example.invalid/signin"
token_url = "http://127.0.0.1:${http.port}/oauth/token"
client_id = "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2"
redirect_uri = "https://connect.garmin.com/modern"
capture = { at = "request", from = "query", param = "ticket" }
on_capture = "close"
timeout = "10s"
[providers.garmin.oauth.exchange]
grant_type = "https://connectapi.garmin.com/di-oauth2-service/oauth/grant/service_ticket"
body = { service_ticket = "{value}", service_url = "https://connect.garmin.com/modern" }
`);
    const created = await sb.jerkAsync(['garmin', 'create', '--force', '--cdp', cdp.wsUrl]);
    assert.equal(created.status, 0, `${created.stderr}\n--- cdp ---\n${cdp.log()}`);
    assert.match(created.stdout, /garmin: signed in/);
    assert.equal(posts.length, 1);
    assert.match(posts[0], /service_ticket=ST-fake-ticket-123/);
    assert.match(posts[0], /client_id=GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2/);
    assert.match(
      posts[0],
      /grant_type=https%3A%2F%2Fconnectapi\.garmin\.com%2Fdi-oauth2-service%2Foauth%2Fgrant%2Fservice_ticket/
    );

    const got = await sb.jerkAsync(['garmin', 'get', '--reveal']);
    assert.equal(got.status, 0, got.stderr);
    assert.match(got.stdout, /garminRefreshedAccessFAKE1234567890/);
    assert.equal(posts.length, 2);
    assert.match(posts[1], /grant_type=refresh_token/);
  } finally {
    await Promise.all([cdp.close(), http.close()]);
  }
});
