import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

function mcpSandbox(servers) {
  const sb = sandbox();
  const agent = join(sb.env.HOME, '.pi', 'agent');
  mkdirSync(agent, { recursive: true });
  writeFileSync(join(agent, 'mcp.json'), JSON.stringify({ mcpServers: servers }));
  return sb;
}

test('a URL-only id is not signed in and has no env variable', () => {
  const sb = mcpSandbox({ other: { url: 'https://other.example.invalid/mcp' } });
  const res = sb.jerk(['mcp:url:HTTPS://MCP.Example.invalid:443/mcp', 'get', '--json'], {
    SLICC_JERK_STORE: 'file',
  });
  assert.equal(res.status, 3, res.stderr);
  assert.equal(
    res.stderr,
    'jerk: mcp:url:https://mcp.example.invalid/mcp: not signed in; run jerk mcp:url:https://mcp.example.invalid/mcp create\n'
  );
  const list = sb.jerk(['list']);
  assert.equal(list.status, 0, list.stderr);
  assert.doesNotMatch(list.stdout, /mcp:url:/);
  assert.match(list.stdout, /\nmcp:other\s/);
});

test('config show resolves a URL id to the mcp.json entry with that URL', () => {
  const sb = mcpSandbox({
    zeta: { url: 'https://mcp.example.invalid/mcp' },
    alpha: { url: 'https://MCP.example.invalid:443/mcp', oauth: { clientId: 'cid' } },
  });
  const res = sb.jerk(['config', 'show', 'mcp:url:https://mcp.example.invalid/mcp']);
  assert.equal(res.status, 0, res.stderr);
  assert.match(res.stdout, /^\[providers\."mcp:alpha"\]\n/);
  assert.match(res.stdout, /client_id = "cid"/);
  assert.match(
    res.stderr,
    /^jerk: mcp:url:https:\/\/mcp\.example\.invalid\/mcp resolves to mcp:alpha, the first by name of the entries with this URL; also mcp:zeta\n/
  );
  const solo = sb.jerk(['config', 'show', 'mcp:url:https://solo.example.invalid']);
  assert.equal(solo.status, 0, solo.stderr);
  assert.match(solo.stdout, /^\[providers\."mcp:url:https:\/\/solo\.example\.invalid\/"\]\n/);
  assert.match(solo.stderr, /is URL-only/);
});

test('bad URL ids exit 2 without echoing userinfo', () => {
  const sb = mcpSandbox({});
  for (const [url, why] of [
    ['https://user:hunter2@mcp.example.invalid/', /userinfo/],
    ['https://mcp.example.invalid/mcp?x=1', /query/],
    ['https://mcp.example.invalid/mcp#x', /fragment/],
    ['http://mcp.example.invalid/mcp', /https, or http on loopback/],
    ['not-a-url', /absolute URL/],
  ]) {
    for (const args of [
      [`mcp:url:${url}`, 'get'],
      ['config', 'show', `mcp:url:${url}`],
    ]) {
      const res = sb.jerk(args);
      assert.equal(res.status, 2, `${args}: ${res.stderr}`);
      assert.match(res.stderr, why);
      assert.doesNotMatch(res.stderr + res.stdout, /hunter2/);
    }
  }
});
