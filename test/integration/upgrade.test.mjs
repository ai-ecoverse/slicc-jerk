import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  realpathSync,
  statSync,
} from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { test } from 'node:test';
import { binary, expectedVersion, sandbox } from './helpers.mjs';

const development = /^0(\.0)*$/.test(expectedVersion) || !/^\d+(\.\d+)*$/.test(expectedVersion);
const os = { darwin: 'darwin', linux: 'linux' }[process.platform];
const arch = { x64: 'amd64', arm64: 'arm64' }[process.arch];
const asset = `jerk-${os}-${arch}`;
const sha256 = (data) => createHash('sha256').update(data).digest('hex');
const refusal =
  'releases are fetched over https, or plain http only from 127.0.0.1, ::1 or localhost';
const script = (reported) => Buffer.from(`#!/bin/sh\necho "jerk ${reported}"\n`);
const skip = !os || !arch ? `no release asset for ${process.platform} ${process.arch}` : false;

function fresh() {
  return {
    tag: 'v9.9.9',
    payload: script('9.9.9'),
    sums: null,
    assetUrl: null,
    sumsUrl: null,
    status: 200,
    downloads: 0,
    requests: [],
  };
}

async function fakeGitHub(t) {
  const state = fresh();
  const server = createServer((req, res) => {
    state.requests.push({ url: req.url, agent: req.headers['user-agent'] });
    if (state.status !== 200) {
      res.writeHead(state.status).end('unavailable');
      return;
    }
    const base = `http://127.0.0.1:${server.address().port}`;
    if (req.url === '/repos/ai-ecoverse/slicc-jerk/releases/latest') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(
        JSON.stringify({
          tag_name: state.tag,
          assets: [
            { name: 'jerk.wasm', browser_download_url: `${base}/dl/jerk.wasm` },
            { name: asset, browser_download_url: state.assetUrl ?? `${base}/dl/${asset}` },
            { name: 'SHA256SUMS', browser_download_url: state.sumsUrl ?? `${base}/dl/SHA256SUMS` },
          ],
        })
      );
      return;
    }
    if (req.url === `/dl/${asset}`) {
      res.writeHead(302, { location: `/objects/${asset}` }).end();
      return;
    }
    if (req.url === `/objects/${asset}`) {
      state.downloads += 1;
      res.writeHead(200, { 'content-type': 'application/octet-stream' });
      res.end(state.payload);
      return;
    }
    if (req.url === '/dl/SHA256SUMS') {
      res.writeHead(200, { 'content-type': 'text/plain' });
      res.end(state.sums ?? `${'0'.repeat(64)}  jerk.wasm\n${sha256(state.payload)}  ${asset}\n`);
      return;
    }
    res.writeHead(404).end();
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  t.after(() => server.close());
  return { state, base: `http://127.0.0.1:${server.address().port}` };
}

function setup() {
  const sb = sandbox();
  let n = 0;
  return {
    sb,
    install(dir = join(sb.dir, `bin-${++n}`)) {
      mkdirSync(dir, { recursive: true });
      const bin = join(dir, 'jerk');
      copyFileSync(binary, bin);
      chmodSync(bin, 0o755);
      return bin;
    },
  };
}

function runAt(bin, args, env) {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, { env });
    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      stdout += chunk;
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk;
    });
    child.on('error', reject);
    child.on('close', (status) => resolve({ status, stdout, stderr }));
  });
}

const notice = (command, base) =>
  `jerk ${command}: SLICC_UPDATE_API_BASE overrides the GitHub API with ${base}\n`;

function after(result, command, base) {
  assert.ok(result.stderr.startsWith(notice(command, base)), result.stderr);
  return result.stderr.slice(notice(command, base).length);
}

function untouched(bin) {
  assert.equal(sha256(readFileSync(bin)), sha256(readFileSync(binary)));
  assert.deepEqual(readdirSync(join(bin, '..')), ['jerk']);
}

test('jerk upgrade replaces the binary with the verified latest release', { skip }, async (t) => {
  const fake = await fakeGitHub(t);
  const { sb, install } = setup();
  const run = (bin, args, base = fake.base, extra = {}) => {
    const env = { ...sb.env, ...extra };
    if (base !== null) env.SLICC_UPDATE_API_BASE = base;
    return runAt(bin, args, env);
  };
  const reset = () => Object.assign(fake.state, fresh());

  await t.test('--check reports the latest release and changes nothing', async () => {
    reset();
    const bin = install();
    const result = await run(bin, ['upgrade', '--check']);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stderr, notice('upgrade', fake.base));
    assert.equal(
      result.stdout,
      development
        ? `latest release: 9.9.9 (this is a development build, ${expectedVersion})\n`
        : `jerk 9.9.9 is available (you have ${expectedVersion}); run \`jerk upgrade\`\n`
    );
    assert.equal(fake.state.downloads, 0);
    assert.match(fake.state.requests[0].agent, /^jerk\//);
    untouched(bin);
  });

  await t.test('update is an alias of upgrade', async () => {
    reset();
    const bin = install();
    const result = await run(bin, ['update', '--check']);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stderr, notice('update', fake.base));
    assert.match(result.stdout, /9\.9\.9/);
    untouched(bin);
  });

  await t.test(
    'a development build is not replaced without --force',
    { skip: development ? false : 'this build has a release version' },
    async () => {
      reset();
      const bin = install();
      const result = await run(bin, ['upgrade']);
      assert.equal(result.status, 2);
      assert.equal(
        after(result, 'upgrade', fake.base),
        `jerk upgrade: refusing to replace a development build (${expectedVersion}) with release 9.9.9; pass --force to replace it\n`
      );
      assert.equal(fake.state.downloads, 0);
      untouched(bin);
    }
  );

  await t.test('update downloads through a redirect and swaps in place', async () => {
    reset();
    const bin = install();
    const result = await run(bin, ['update', '--force']);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stderr, notice('update', fake.base));
    assert.equal(
      result.stdout,
      `upgrading jerk ${expectedVersion} to 9.9.9\nupgraded ${realpathSync(bin)} to jerk 9.9.9 (SHA256SUMS verified)\n`
    );
    assert.equal(fake.state.downloads, 1);
    assert.ok(fake.state.requests.some((r) => r.url === `/objects/${asset}`));
    assert.equal(readFileSync(bin, 'utf8'), script('9.9.9').toString());
    assert.ok(statSync(bin).mode & 0o100);
    assert.deepEqual(readdirSync(join(bin, '..')), ['jerk']);
  });

  await t.test('a real release binary passes the version gate', async () => {
    reset();
    fake.state.tag = `v${expectedVersion}`;
    fake.state.payload = readFileSync(binary);
    const bin = install();
    const before = statSync(bin).ino;
    const result = await run(bin, ['upgrade', '--force']);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(fake.state.downloads, 1);
    assert.notEqual(statSync(bin).ino, before);
    untouched(bin);
  });

  await t.test('a checksum mismatch replaces nothing', async () => {
    reset();
    fake.state.sums = `${'ab'.repeat(32)}  ${asset}\n`;
    const bin = install();
    const result = await run(bin, ['upgrade', '--force']);
    assert.equal(result.status, 1);
    assert.equal(
      after(result, 'upgrade', fake.base),
      `jerk upgrade: ${asset} does not match its SHA-256 in SHA256SUMS\n`
    );
    untouched(bin);
  });

  await t.test('an asset missing from SHA256SUMS is refused before it is downloaded', async () => {
    reset();
    fake.state.sums = `${'0'.repeat(64)}  jerk.wasm\n`;
    const bin = install();
    const result = await run(bin, ['upgrade', '--force']);
    assert.equal(result.status, 1);
    assert.equal(
      after(result, 'upgrade', fake.base),
      `jerk upgrade: SHA256SUMS has no entry for ${asset}\n`
    );
    assert.equal(fake.state.downloads, 0);
    untouched(bin);
  });

  await t.test('a binary that reports another version replaces nothing', async () => {
    reset();
    fake.state.payload = script('9.9.8');
    const bin = install();
    const result = await run(bin, ['upgrade', '--force']);
    assert.equal(result.status, 1);
    assert.equal(
      after(result, 'upgrade', fake.base),
      'jerk upgrade: the downloaded binary reports "jerk 9.9.8", expected "jerk 9.9.9"\n'
    );
    untouched(bin);
  });

  await t.test('a binary that does not run replaces nothing', async () => {
    reset();
    fake.state.payload = Buffer.from('not a binary');
    const bin = install();
    const result = await run(bin, ['upgrade', '--force']);
    assert.equal(result.status, 1);
    assert.match(
      after(result, 'upgrade', fake.base),
      /^jerk upgrade: the downloaded binary failed to run --version/
    );
    untouched(bin);
  });

  await t.test(
    'a binary or directory that is not writable is not downloaded or replaced',
    { skip: process.getuid?.() === 0 ? 'root can write anything' : false },
    async () => {
      reset();
      const bin = install();
      chmodSync(bin, 0o555);
      const file = await run(bin, ['upgrade', '--force']);
      assert.equal(file.status, 1);
      assert.equal(
        after(file, 'upgrade', fake.base),
        `jerk upgrade: ${realpathSync(bin)} is not writable; rerun with permission to write it, or reinstall with install.sh\n`
      );
      chmodSync(bin, 0o755);
      const dir = join(bin, '..');
      chmodSync(dir, 0o555);
      try {
        const locked = await run(bin, ['upgrade', '--force']);
        assert.equal(locked.status, 1);
        assert.equal(
          after(locked, 'upgrade', fake.base),
          `jerk upgrade: ${realpathSync(dir)} is not writable, so ${realpathSync(bin)} can't be replaced; rerun with permission to write it, or reinstall with install.sh\n`
        );
        const check = await run(bin, ['upgrade', '--check']);
        assert.equal(check.status, 0, check.stderr);
      } finally {
        chmodSync(dir, 0o755);
      }
      assert.equal(fake.state.downloads, 0);
      untouched(bin);
    }
  );

  await t.test('managed installs are left to their tool', async () => {
    reset();
    const cargoHome = join(sb.dir, 'cargo-home');
    const cargo = install(join(cargoHome, 'bin'));
    const cargoAdvice =
      'jerk upgrade: jerk was installed with cargo; update it with `cargo install --git https://github.com/ai-ecoverse/slicc-jerk jerk`\n';
    const overridden = await run(cargo, ['upgrade'], fake.base, { CARGO_HOME: cargoHome });
    assert.equal(overridden.status, 2);
    assert.equal(overridden.stderr, notice('upgrade', fake.base) + cargoAdvice);
    const plain = await run(cargo, ['upgrade', '--check'], null, { CARGO_HOME: cargoHome });
    assert.equal(plain.status, 2);
    assert.equal(plain.stderr, cargoAdvice);
    const home = install(join(sb.env.HOME, '.cargo', 'bin'));
    const fromHome = await run(home, ['upgrade', '--force'], null);
    assert.equal(fromHome.stderr, cargoAdvice);

    const npmAdvice =
      'jerk upgrade: jerk was installed with npm; update it with `npm install -g @ai-ecoverse/slicc-jerk@latest`, or in the project that depends on it\n';
    const npm = install(
      join(
        sb.dir,
        'lib',
        'node_modules',
        '@ai-ecoverse',
        `slicc-jerk-${process.platform}-${process.arch}`,
        'bin'
      )
    );
    const fromNpm = await run(npm, ['upgrade', '--force'], null);
    assert.equal(fromNpm.status, 2);
    assert.equal(fromNpm.stderr, npmAdvice);
    const launched = await run(install(), ['upgrade', '--force'], null, { JERK_LAUNCHER: 'npm' });
    assert.equal(launched.status, 2);
    assert.equal(launched.stderr, npmAdvice);
    const npx = install(join(sb.dir, 'cache', '_npx', 'abc', 'node_modules', '.bin'));
    const fromNpx = await run(npx, ['upgrade'], null);
    assert.equal(
      fromNpx.stderr,
      'jerk upgrade: jerk runs from npx; run `npx @ai-ecoverse/slicc-jerk@latest` for the latest\n'
    );

    const data =
      process.platform === 'darwin'
        ? join(sb.env.HOME, 'Library', 'Application Support')
        : join(sb.dir, 'data');
    const slicc = install(join(data, 'slicc', 'bin'));
    const fromSlicc = await run(slicc, ['update', '--force'], null, {
      XDG_DATA_HOME: join(sb.dir, 'data'),
    });
    assert.equal(fromSlicc.status, 2);
    assert.equal(
      fromSlicc.stderr,
      'jerk update: jerk was installed by the slicc CLI; update it with `slicc install jerk`\n'
    );
    assert.equal(fake.state.requests.length, 0);
  });

  await t.test('a non-loopback http API base is refused before it is dialled', async () => {
    reset();
    const bin = install();
    const base = 'http://api.example.com';
    const result = await run(bin, ['upgrade', '--check'], base);
    assert.equal(result.status, 1);
    assert.equal(
      after(result, 'upgrade', base),
      `jerk upgrade: asking GitHub for the latest release: refusing http://api.example.com: ${refusal}\n`
    );
    untouched(bin);
  });

  await t.test('http download URLs on another host are refused', async () => {
    reset();
    fake.state.assetUrl = `http://downloads.example.com/dl/${asset}`;
    const bin = install();
    const result = await run(bin, ['upgrade', '--force']);
    assert.equal(result.status, 1);
    assert.equal(
      after(result, 'upgrade', fake.base),
      `jerk upgrade: downloading ${asset}: refusing http://downloads.example.com: ${refusal}\n`
    );
    reset();
    fake.state.sumsUrl = 'http://downloads.example.com/dl/SHA256SUMS';
    const sums = await run(bin, ['update', '--force']);
    assert.equal(sums.status, 1);
    assert.equal(
      after(sums, 'update', fake.base),
      `jerk update: downloading SHA256SUMS: refusing http://downloads.example.com: ${refusal}\n`
    );
    assert.equal(fake.state.downloads, 0);
    untouched(bin);
  });

  await t.test('a GitHub API failure is reported', async () => {
    reset();
    fake.state.status = 403;
    const bin = install();
    const result = await run(bin, ['upgrade', '--check']);
    assert.equal(result.status, 1);
    assert.equal(
      after(result, 'upgrade', fake.base),
      'jerk upgrade: asking GitHub for the latest release: HTTP 403\n'
    );
    untouched(bin);
  });

  await t.test('unknown arguments exit 2 and --help documents both names', async () => {
    reset();
    const bin = install();
    const bad = await run(bin, ['upgrade', '--nope']);
    assert.equal(bad.status, 2);
    assert.match(bad.stderr, /jerk upgrade: unexpected --nope/);
    for (const name of ['upgrade', 'update']) {
      const help = await run(bin, [name, '--help']);
      assert.equal(help.status, 0);
      assert.match(help.stdout, new RegExp(`^jerk ${name} \\[--check\\] \\[--force\\]\\n`));
    }
    const global = await run(bin, ['--help']);
    assert.match(global.stdout, /jerk upgrade \[--check\] \[--force\] +\(alias: jerk update\)/);
    assert.equal(fake.state.requests.length, 0);
    untouched(bin);
  });
});

test('upgrade and update are reserved, so no provider can use them', () => {
  const sb = sandbox();
  sb.writeUser(
    '[providers.upgrade]\nenv = ["UPGRADE_TOKEN"]\nget = { run = ["x"] }\n[providers.update]\nenv = ["UPDATE_TOKEN"]\nget = { run = ["x"] }\n'
  );
  const res = sb.jerk(['config', 'check']);
  assert.equal(res.status, 2);
  assert.match(
    res.stderr,
    /upgrade: upgrade is a jerk command, so no provider can use it as its id/
  );
  assert.match(res.stderr, /update: update is a jerk command, so no provider can use it as its id/);
});
