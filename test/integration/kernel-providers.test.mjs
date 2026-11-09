import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createNodeKernel } from '@ai-ecoverse/slicc-kernel/node';
import { wasm } from './helpers.mjs';

const skip = !existsSync(wasm) && 'run npm run build:wasm first';
const placeholder = 'fnox_proxy_111111111111111111111111';
const fake = 'fake-gh-token-never-print-me';

async function seven({ stored = false, refused = false, removed = false, host = false } = {}) {
  const seen = [];
  const kernel = await createNodeKernel({
    sealed: async (req) => {
      seen.push(req);
      if (host && req.jop === 'list')
        return {
          v: 1,
          exit: 0,
          rows: [
            { provider: 'github', env: ['GH_TOKEN'], device: 'fixture' },
            { provider: 'linkedin', env: ['LINKEDIN_ACCESS_TOKEN'], device: 'fixture' },
          ],
        };
      if (host && req.jop === 'create') return { v: 1, exit: 4, message: 'stub host unavailable' };
      if (req.op === 'placeholder' && stored)
        return { v: 1, placeholder, state: removed ? 'removed' : 'active' };
      if (req.op === 'seal') {
        if (refused) return { v: 1, error: { code: 'EEXIST', message: fake } };
        assert.equal(req.value, fake);
        stored = true;
        return { v: 1, placeholder, state: 'ready' };
      }
      return { v: 1, exit: 3 };
    },
  });
  const pkg = '/node_modules/@ai-ecoverse/slicc-jerk';
  await kernel.writeFile(
    `${pkg}/package.json`,
    JSON.stringify({
      name: '@ai-ecoverse/slicc-jerk',
      slicc: {
        abi: 'wasi',
        commands: { jerk: { wasm: 'jerk.wasm', imports: 'kernel-process.mjs' } },
      },
    })
  );
  await kernel.writeFile(`${pkg}/jerk.wasm`, readFileSync(wasm));
  await kernel.writeFile(
    `${pkg}/kernel-process.mjs`,
    readFileSync(new URL('../../npm/kernel-process.mjs', import.meta.url))
  );
  const gh = '/node_modules/fake-gh';
  await kernel.writeFile(
    `${gh}/package.json`,
    JSON.stringify({
      name: 'fake-gh',
      slicc: { abi: 'js', commands: { gh: { module: 'gh.mjs' } } },
    })
  );
  await kernel.writeFile(
    `${gh}/gh.mjs`,
    `export default async function(ctx) {
    await ctx.fs.writeFile('/tmp/gh-call', JSON.stringify({ argv: ctx.argv, env: ctx.env }));
    await ctx.write(2, '${fake}');
    await ctx.write(1, '${fake}\\n');
    return 0;
  }`
  );
  return { kernel, seen };
}

test('kernel builtins use the kernel gh and seal its output, also with --reveal', {
  skip,
}, async () => {
  const { kernel, seen } = await seven();
  try {
    const list = await kernel.run(['jerk', 'list', '--json']);
    assert.equal(list.status, 0, list.stderr);
    assert.equal(JSON.parse(list.stdout).find((p) => p.provider === 'github').cli, 'gh');
    const res = await kernel.run(['jerk', 'github', 'get', '--reveal', '-p', 'host=example.test']);
    assert.equal(res.status, 0, res.stderr);
    assert.equal(res.stdout, `${placeholder}\n`);
    assert.doesNotMatch(res.stderr, /fake-gh/);
    const call = JSON.parse(new TextDecoder().decode(await kernel.readFile('/tmp/gh-call')));
    assert.deepEqual(call.argv, ['gh', 'auth', 'token', '--hostname', 'example.test']);
    assert.equal(seen.find((req) => req.op === 'seal').name, 'GH_TOKEN');
    const next = await kernel.run(['jerk', 'github', 'get', '--json']);
    assert.deepEqual(JSON.parse(next.stdout), {
      provider: 'github',
      source: 'sealed',
      placeholder,
    });
  } finally {
    kernel.terminate();
  }
});

test('builtin OAuth create preserves linked-host routing and params', { skip }, async () => {
  const { kernel, seen } = await seven({ host: true });
  try {
    const res = await kernel.run([
      'jerk',
      'linkedin',
      'create',
      '-p',
      'client_id=fake-client',
      '-p',
      'client_secret=fake-secret',
    ]);
    assert.equal(res.status, 4, res.stderr);
    const request = seen.find((req) => req.jop === 'create');
    assert.equal(request.kind, 'jerk');
    assert.equal(request.provider, 'linkedin');
    assert.deepEqual(request.params, { client_id: 'fake-client', client_secret: 'fake-secret' });
    assert.equal(
      seen.some((req) => req.kind === 'oauth'),
      false
    );
  } finally {
    kernel.terminate();
  }
});

test('kernel catalog retains host availability for duplicate providers', { skip }, async () => {
  const { kernel } = await seven({ host: true });
  try {
    const res = await kernel.run(['jerk', 'list', '--json']);
    assert.equal(res.status, 0, res.stderr);
    const rows = JSON.parse(res.stdout).filter((row) => row.provider === 'github');
    assert.deepEqual(
      rows.map((row) => [row.source, row.cli]),
      [
        ['seven', 'gh'],
        ['host:fixture', 'host'],
      ]
    );
    const text = await kernel.run(['jerk', 'list']);
    assert.match(text.stdout, /host:fixture/);
  } finally {
    kernel.terminate();
  }
});

test('kernel get gives env precedence over vault and vault precedence over CLI', {
  skip,
}, async () => {
  const { kernel, seen } = await seven({ stored: true });
  try {
    const fromVault = await kernel.run(['jerk', 'github', 'get']);
    assert.equal(fromVault.status, 0, fromVault.stderr);
    assert.equal(fromVault.stdout, `${placeholder}\n`);
    await assert.rejects(kernel.readFile('/tmp/gh-call'), { code: 'ENOENT' });
    const fromEnv = await kernel.run(['jerk', 'github', 'get', '--json'], {
      env: { GH_TOKEN: '', GITHUB_TOKEN: fake },
    });
    assert.equal(fromEnv.status, 0, fromEnv.stderr);
    assert.deepEqual(JSON.parse(fromEnv.stdout), {
      provider: 'github',
      source: 'env',
      placeholder,
    });
    assert.equal(seen.at(-1).name, 'GITHUB_TOKEN');
  } finally {
    kernel.terminate();
  }
});

test('kernel get does not reuse a removed vault record', { skip }, async () => {
  const { kernel, seen } = await seven({ stored: true, removed: true });
  try {
    const res = await kernel.run(['jerk', 'github', 'get']);
    assert.equal(res.status, 0, res.stderr);
    assert.equal(res.stdout, `${placeholder}\n`);
    assert.ok(seen.some((req) => req.op === 'seal'));
  } finally {
    kernel.terminate();
  }
});

test('a refused seal never falls back to exposing the CLI token', { skip }, async () => {
  const { kernel } = await seven({ refused: true });
  try {
    const res = await kernel.run(['jerk', 'github', 'get', '--reveal']);
    assert.equal(res.status, 1, res.stderr);
    assert.equal(res.stdout, '');
    assert.match(res.stderr, /EEXIST/);
    assert.doesNotMatch(res.stderr, /fake-gh/);
  } finally {
    kernel.terminate();
  }
});
