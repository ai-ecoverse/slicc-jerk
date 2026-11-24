import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { test } from 'node:test';
import { stubCdp } from './cdp-stub.mjs';
import { wasm } from './helpers.mjs';
import { seven } from './kernel-oauth-harness.mjs';

const skip = !existsSync(wasm) && 'run npm run build:wasm first';
const redirect = 'http://localhost:43819/callback';
function failure(url, id, options = {}) {
  const frameId = options.child ? 'child' : 'main';
  const params = { requestId: id, type: 'Document' };
  return [
    {
      method: 'Network.requestWillBeSent',
      params: {
        ...params,
        frameId,
        request: { method: options.method ?? 'GET', url, headers: options.headers ?? {} },
      },
    },
    {
      method: 'Network.loadingFailed',
      params: { ...params, errorText: options.error ?? 'net::ERR_CONNECTION_REFUSED' },
    },
    {
      method: 'Page.frameNavigated',
      params: {
        frame: {
          id: frameId,
          ...(options.child ? { parentId: 'main' } : {}),
          loaderId: id,
          url: 'chrome-error://chromewebdata/',
          securityOrigin: '://',
          unreachableUrl: url,
        },
      },
    },
  ];
}

for (const [at, wrongState] of [
  ['request', false],
  ['navigation', false],
  ['callback', true],
]) {
  test(`kernel ${at} accepts only a matching main-frame failed GET, wrongState=${wrongState}`, {
    skip,
  }, async () => {
    const browser = await stubCdp({
      navigated: (authorize) => {
        const state = new URL(authorize).searchParams.get('state');
        const decoy = `${redirect}?code=decoy&state=${state}`;
        const url = `${redirect}?code=FAKE-failed-code&state=${wrongState ? 'wrong' : state}`;
        return [
          ...failure(decoy, '1', { child: true }),
          ...failure(decoy, '2', { method: 'POST' }),
          ...failure(decoy, '3', { error: 'net::ERR_BLOCKED_BY_CLIENT' }),
          ...failure(decoy.replace('/callback?', '/callback-other?'), '4'),
          ...failure(decoy, '5', { headers: { Origin: 'https://other.test' } }),
          ...failure(decoy, '6').map((e) => ({ ...e, sessionId: 'another-session' })),
          ...failure(url, '7'),
        ];
      },
    });
    const config = `[providers.demo]
env = ["DEMO_TOKEN"]
[providers.demo.oauth]
mode = "auto"
flow = "pkce"
authorize_url = "https://idp.example.test/authorize?redirect_uri={redirect_uri}&state={state}&code_challenge={code_challenge}"
token_url = "https://idp.example.test/token"
client_id = "test"
redirect_uri = "${redirect}"
capture = { at = "${at}", from = "query", param = "code" }
timeout = "2s"
`;
    const { kernel, run, posts, seals } = await seven('auto', { config, browser });
    try {
      const res = await run('create');
      assert.equal(res.status, wrongState ? 1 : 0, res.stderr);
      assert.match(res.stderr, /callback via CDP/);
      assert.doesNotMatch(res.stdout + res.stderr, /FAKE-failed-code|decoy/);
      assert.match(browser.log(), /Target.closeTarget/);
      if (wrongState) {
        assert.match(res.stderr, /state mismatch/);
        assert.equal(posts.length, 0);
        assert.equal(seals.length, 0);
      } else {
        assert.equal(posts.length, 1);
        const form = new URLSearchParams(posts[0].body);
        assert.equal(form.get('code'), 'FAKE-failed-code');
        assert.equal(form.get('redirect_uri'), redirect);
        assert.ok(form.get('code_verifier'));
        assert.equal(seals.length, 1);
      }
    } finally {
      kernel.terminate();
      await browser.close();
    }
  });
}
