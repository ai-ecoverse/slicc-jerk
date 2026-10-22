import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { binary, expectedVersion, root } from './helpers.mjs';

const skip = process.platform === 'win32' && 'install.sh is for macOS and Linux';
const os = { darwin: 'darwin', linux: 'linux' }[process.platform];
const arch = { arm64: 'arm64', x64: 'amd64' }[process.arch];
const asset = `jerk-${os}-${arch}`;
const sha = (data) => createHash('sha256').update(data).digest('hex');
let work;
let server;
let base;
const releases = new Map();

function release(path, files) {
  releases.set(path, files);
}

function run(env, args = [], prefix = '') {
  return new Promise((resolve) => {
    const child = spawn('/bin/sh', [join(root, 'install.sh'), ...args], {
      env: { HOME: join(work, 'home'), ...env, SLICC_INSTALL_BASE_URL: `${base}${prefix}` },
    });
    let out = '';
    child.stdout.on('data', (chunk) => {
      out += chunk;
    });
    child.stderr.on('data', (chunk) => {
      out += chunk;
    });
    child.on('close', (status) => resolve({ status, out }));
  });
}

before(async () => {
  if (skip) return;
  work = mkdtempSync(join(tmpdir(), 'jerk-install-'));
  const bytes = readFileSync(binary);
  const good = { [asset]: bytes, SHA256SUMS: `${sha(bytes)}  ${asset}\n` };
  release('/latest/download', good);
  release('/download/v9.8.7', good);
  release('/tampered/latest/download', { ...good, [asset]: Buffer.from('tampered') });
  release('/missing/latest/download', { ...good, SHA256SUMS: `${sha(bytes)}  jerk-none\n` });
  server = createServer((req, res) => {
    const at = req.url.lastIndexOf('/');
    const body = releases.get(req.url.slice(0, at))?.[req.url.slice(at + 1)];
    res.writeHead(body === undefined ? 404 : 200);
    res.end(body);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  base = `http://127.0.0.1:${server.address().port}`;
});

after(() => {
  server?.close();
  if (work) rmSync(work, { recursive: true, force: true });
});

test('install.sh installs the release asset for this host after checking SHA256SUMS', {
  skip,
}, async () => {
  const dir = join(work, 'curl');
  const res = await run({ PATH: process.env.PATH, INSTALL_DIR: dir });
  assert.equal(res.status, 0, res.out);
  assert.match(res.out, new RegExp(`jerk ${expectedVersion.replaceAll('.', '\\.')}`));
  assert.match(res.out, /Add .* to your PATH/);
  assert.ok(readFileSync(join(dir, 'jerk')).equals(readFileSync(binary)));
  const again = await run({ PATH: `${dir}:${process.env.PATH}` }, ['--dir', dir]);
  assert.equal(again.status, 0, again.out);
  assert.doesNotMatch(again.out, /to your PATH/);
});

test('install.sh --version downloads that tag', { skip }, async () => {
  const dir = join(work, 'pinned');
  const res = await run({ PATH: process.env.PATH }, ['--version', '9.8.7', '--dir', dir]);
  assert.equal(res.status, 0, res.out);
  assert.ok(existsSync(join(dir, 'jerk')));
});

test('install.sh works with wget when there is no curl', { skip }, async () => {
  const bin = join(work, 'wget-bin');
  mkdirSync(bin);
  for (const tool of [
    'uname',
    'mktemp',
    'grep',
    'cut',
    'sha256sum',
    'shasum',
    'mkdir',
    'cp',
    'chmod',
    'mv',
    'rm',
  ]) {
    const found = spawnSync('sh', ['-c', `command -v ${tool}`], { encoding: 'utf8' }).stdout.trim();
    if (found) symlinkSync(found, join(bin, tool));
  }
  writeFileSync(
    join(bin, 'wget'),
    `#!${process.execPath}\nconst [, , flags, file, url] = process.argv;\nif (flags !== '-qO') process.exit(2);\nfetch(url).then(async (res) => { if (!res.ok) process.exit(8); require('node:fs').writeFileSync(file, Buffer.from(await res.arrayBuffer())); });\n`
  );
  chmodSync(join(bin, 'wget'), 0o755);
  assert.ok(!existsSync(join(bin, 'curl')));
  const dir = join(work, 'wget');
  const res = await run({ PATH: bin, INSTALL_DIR: dir });
  assert.equal(res.status, 0, res.out);
  assert.ok(readFileSync(join(dir, 'jerk')).equals(readFileSync(binary)));
});

test('install.sh refuses an asset whose checksum does not match', { skip }, async () => {
  const dir = join(work, 'tampered');
  const res = await run({ PATH: process.env.PATH }, ['--dir', dir], '/tampered');
  assert.notEqual(res.status, 0);
  assert.match(res.out, /checksum mismatch/);
  assert.ok(!existsSync(join(dir, 'jerk')));
});

test('install.sh refuses an asset that SHA256SUMS does not list', { skip }, async () => {
  const dir = join(work, 'missing');
  const res = await run({ PATH: process.env.PATH }, ['--dir', dir], '/missing');
  assert.notEqual(res.status, 0);
  assert.match(res.out, new RegExp(`no checksum for ${asset}`));
  assert.ok(!existsSync(join(dir, 'jerk')));
});
