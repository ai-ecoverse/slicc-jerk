import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createNodeKernel } from '@ai-ecoverse/slicc-kernel/node';
import { wasm } from './helpers.mjs';

const skip = !existsSync(wasm) && 'run npm run build:wasm first';
const settings = 'sign-in happens in Settings › MCP servers, where it is sealed';
const server = 'https://mcp.example.test/mcp';
const noLink = 'no linked device runs jerk: link a slicc CLI on a computer that has jerk';
const refused = 'create needs a provider matching ^[a-z0-9][a-z0-9-]{0,63}$';

function vault(answer) {
  const seen = [];
  const replies = new Map();
  const sealed = async (req) => {
    if (req.op === 'request') {
      seen.push(req);
      const id = `req-${seen.length}`;
      replies.set(id, answer(req));
      return { v: 1, state: 'pending', id };
    }
    return { v: 1, state: 'done', id: req.id, ...replies.get(req.id) };
  };
  return { sealed, seen };
}

function bios(req) {
  if (req.jop === 'list') return { exit: 0, rows: [] };
  if (req.jop === 'status') {
    return {
      exit: 3,
      message: `no token for ${req.provider} in seven's vault: run jerk ${req.provider} create`,
    };
  }
  return { exit: 4, message: noLink };
}

async function seven(sealed) {
  const kernel = await createNodeKernel({ sealed });
  const pkg = '/node_modules/@ai-ecoverse/slicc-jerk';
  await kernel.writeFile(
    `${pkg}/package.json`,
    JSON.stringify({
      name: '@ai-ecoverse/slicc-jerk',
      slicc: { abi: 'wasi', commands: { jerk: { wasm: 'bin/jerk.wasm' } } },
    })
  );
  await kernel.writeFile(`${pkg}/bin/jerk.wasm`, readFileSync(wasm));
  await kernel.writeFile('/tmp/mcp.json', JSON.stringify({ mcpServers: { bb: { url: server } } }));
  await kernel.writeFile(
    '/tmp/providers.toml',
    '[providers.demo]\nname = "Demo"\nenv = ["DEMO_TOKEN"]\nrequires = "demo"\nget = { run = ["demo", "token"] }\n'
  );
  return kernel;
}

const jerk = (kernel, ...args) =>
  kernel.run(['jerk', ...args], {
    env: { PI_MCP_CONFIG: '/tmp/mcp.json', SLICC_JERK_CONFIG: '/tmp/providers.toml' },
  });

for (const [id, shown] of [
  ['mcp:bb', 'mcp:bb'],
  [`mcp:url:${server}`, 'mcp:bb'],
  ['mcp:url:https://other.example.test/mcp', 'mcp:url:https://other.example.test/mcp'],
]) {
  test(`with no linked CLI, ${id} sends sign-in to Settings`, { skip }, async () => {
    const kernel = await seven(vault(bios).sealed);
    try {
      const get = await jerk(kernel, id, 'get');
      assert.equal(get.status, 3, get.stderr);
      assert.equal(get.stderr, `jerk: ${shown}: not signed in\njerk: ${shown}: ${settings}\n`);

      const create = await jerk(kernel, id, 'create');
      assert.equal(create.status, 4, create.stderr);
      assert.equal(create.stderr, `jerk: ${shown}: ${noLink}\njerk: ${shown}: ${settings}\n`);
    } finally {
      kernel.terminate();
    }
  });
}

test('a record sealed in Settings is what get prints after the hint', { skip }, async () => {
  const placeholder = 'fnox_proxy_000000000000000000000000';
  let signedIn = false;
  const { sealed } = vault((req) => (signedIn ? { exit: 0, placeholder } : bios(req)));
  const kernel = await seven(sealed);
  try {
    const before = await jerk(kernel, 'mcp:bb', 'get');
    assert.equal(before.status, 3, before.stderr);
    assert.match(before.stderr, /Settings › MCP servers/);
    signedIn = true;
    const after = await jerk(kernel, 'mcp:bb', 'get', '--reveal');
    assert.equal(after.status, 0, after.stderr);
    assert.equal(after.stdout, `${placeholder}\n`);
  } finally {
    kernel.terminate();
  }
});

test('a failed sign-in on the linked machine also names Settings', { skip }, async () => {
  const { sealed, seen } = vault(() => ({ exit: 1, message: refused }));
  const kernel = await seven(sealed);
  try {
    const res = await jerk(kernel, 'mcp:bb', 'create');
    assert.equal(res.status, 1, res.stderr);
    assert.equal(res.stderr, `jerk: mcp:bb: ${refused}\njerk: mcp:bb: ${settings}\n`);
    assert.deepEqual(
      seen.map((req) => [req.kind, req.jop, req.provider]),
      [['jerk', 'create', 'mcp:bb']]
    );
  } finally {
    kernel.terminate();
  }
});

test('only MCP hints move to Settings', { skip }, async () => {
  const kernel = await seven(vault(() => ({ exit: 3, message: 'no token' })).sealed);
  try {
    const mcp = await jerk(kernel, 'mcp:bb', 'get');
    assert.equal(mcp.stderr, `jerk: mcp:bb: not signed in\njerk: mcp:bb: ${settings}\n`);
    const demo = await jerk(kernel, 'demo', 'get');
    assert.equal(demo.status, 3, demo.stderr);
    assert.equal(demo.stderr, 'jerk: demo: no token\njerk: demo: sign in with jerk demo create\n');
  } finally {
    kernel.terminate();
  }
});
