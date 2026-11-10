import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { createNodeKernel } from '@ai-ecoverse/slicc-kernel/node';
import { sandbox, wasm } from './helpers.mjs';

const skip = !existsSync(wasm) && 'run npm run build:wasm first';
const placeholder = 'fnox_proxy_555555555555555555555555';
const fake = 'fake-token-must-not-appear';
const config = `[providers.demo]
env = ["DEMO_TOKEN"]
requires = "fake-cli"
get = { run = ["fake-cli", "token"], parse = "text" }
list = { run = ["fake-cli", "list"], parse = "json:" }
`;
const cliRows = [{ id: 'first', name: 'work', token: fake }];

async function seven({ vault = true, cli = true, unavailable = false, cliExit = 0 } = {}) {
  const seen = [];
  const kernel = await createNodeKernel({
    sealed: async (req) => {
      seen.push(req);
      if (unavailable) return { v: 1, error: { code: 'EACCES', message: fake } };
      return {
        v: 1,
        secrets: vault
          ? [
              { name: 'OTHER_TOKEN', placeholder, state: 'active' },
              { name: 'DEMO_TOKEN', placeholder, state: 'removed' },
              {
                name: 'DEMO_TOKEN',
                placeholder,
                state: 'inert',
                exec: false,
                rules: [],
                value: fake,
              },
            ]
          : [],
      };
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
  await kernel.writeFile(
    '/tmp/providers.toml',
    cli ? config : config.replace('list = { run = ["fake-cli", "list"], parse = "json:" }\n', '')
  );
  if (cli) {
    const path = '/node_modules/fake-cli';
    await kernel.writeFile(
      `${path}/package.json`,
      JSON.stringify({
        name: 'fake-cli',
        slicc: { abi: 'js', commands: { 'fake-cli': { module: 'fake.mjs' } } },
      })
    );
    await kernel.writeFile(
      `${path}/fake.mjs`,
      cliExit === 0
        ? `export default async function(ctx) {
      await ctx.write(2, '${fake}');
      await ctx.write(1, ${JSON.stringify(JSON.stringify(cliRows))});
      return 0;
    }`
        : `export default async function(ctx) {
      await ctx.write(2, 'fake-cli list failed\\n');
      return ${cliExit};
    }`
    );
  }
  const run = (...args) =>
    kernel.run(['jerk', 'demo', 'list', ...args], {
      env: { SLICC_JERK_CONFIG: '/tmp/providers.toml' },
    });
  return { kernel, run, seen };
}

test('kernel provider list combines vault metadata and redacted CLI items', { skip }, async () => {
  const { kernel, run, seen } = await seven();
  try {
    const res = await run('--json');
    assert.equal(res.status, 0, res.stderr);
    const { items } = JSON.parse(res.stdout);
    assert.equal(items.length, 2);
    assert.equal(items[0].id, 'first');
    assert.deepEqual(items[1], {
      name: 'DEMO_TOKEN',
      placeholder,
      state: 'inert',
      exec: false,
      rules: [],
      source: 'sealed',
    });
    assert.doesNotMatch(res.stdout + res.stderr, /fake-token-must-not-appear/);
    assert.deepEqual(
      seen.map((r) => r.op),
      ['list']
    );
    const text = await run();
    assert.deepEqual(JSON.parse(text.stdout), items);
    assert.equal((await run('--bogus')).status, 2);
  } finally {
    kernel.terminate();
  }
});

test('kernel CLI-only provider list matches native text and JSON formats', { skip }, async () => {
  const { kernel, run } = await seven({ vault: false });
  const sb = sandbox();
  sb.writeUser(config);
  sb.script('fake-cli', `process.stdout.write(${JSON.stringify(JSON.stringify(cliRows))});`);
  try {
    for (const args of [[], ['--json']]) {
      const native = sb.jerk(['demo', 'list', ...args]);
      const res = await run(...args);
      assert.equal(res.status, 0, res.stderr);
      assert.equal(native.status, 0, native.stderr);
      assert.equal(res.stdout, native.stdout);
      assert.doesNotMatch(res.stdout + res.stderr, /fake-token-must-not-appear/);
    }
  } finally {
    kernel.terminate();
  }
});

for (const options of [
  { cli: false },
  { cli: false, vault: false },
  { cli: false, unavailable: true },
]) {
  test(`kernel vault-only provider list: ${JSON.stringify(options)}`, { skip }, async () => {
    const { kernel, run } = await seven(options);
    try {
      const res = await run('--json');
      assert.equal(res.status, options.unavailable ? 4 : 0, res.stderr);
      assert.doesNotMatch(res.stdout + res.stderr, /fake-token-must-not-appear/);
      if (!options.unavailable)
        assert.equal(JSON.parse(res.stdout).items.length, options.vault === false ? 0 : 1);
    } finally {
      kernel.terminate();
    }
  });
}

test('kernel empty vault success keeps listing usable when the CLI fails', { skip }, async () => {
  const { kernel, run } = await seven({ vault: false, cliExit: 1 });
  try {
    const res = await run('--json');
    assert.equal(res.status, 0, res.stderr);
    assert.deepEqual(JSON.parse(res.stdout), { items: [] });
    assert.doesNotMatch(res.stdout + res.stderr, /fake-token-must-not-appear/);
  } finally {
    kernel.terminate();
  }
});

test('kernel failed vault and failed CLI keep the CLI error', { skip }, async () => {
  const { kernel, run } = await seven({ unavailable: true, cliExit: 1 });
  try {
    const res = await run('--json');
    assert.equal(res.status, 1, res.stderr);
    assert.equal(res.stdout, '');
    assert.doesNotMatch(res.stdout + res.stderr, /fake-token-must-not-appear/);
  } finally {
    kernel.terminate();
  }
});
