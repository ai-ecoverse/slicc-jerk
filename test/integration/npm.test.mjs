import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import {
  copyFileSync,
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { binary, expectedVersion, root } from './helpers.mjs';

const target = `${process.platform}-${process.arch}`;
const goName = { darwin: 'darwin', linux: 'linux', win32: 'windows' }[process.platform];
const goArch = { arm64: 'arm64', x64: 'amd64' }[process.arch];
const asset = `jerk-${goName}-${goArch}${process.platform === 'win32' ? '.exe' : ''}`;
let work;

before(() => {
  work = mkdtempSync(join(tmpdir(), 'jerk-npm-'));
  const from = join(work, 'release');
  mkdirSync(from);
  copyFileSync(binary, join(from, asset));
  writeFileSync(join(from, 'jerk.wasm'), Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]));
  const res = spawnSync(
    process.execPath,
    [
      'scripts/npm-stage.mjs',
      '--version',
      'v1.2.3',
      '--from',
      from,
      '--out',
      join(work, 'npm'),
      '--only',
      target,
    ],
    { cwd: root, encoding: 'utf8' }
  );
  assert.equal(res.status, 0, res.stderr);
});

after(() => rmSync(work, { recursive: true, force: true }));

test('the main package carries the launcher, the wasm and the kernel command', () => {
  const pkg = JSON.parse(readFileSync(join(work, 'npm', 'slicc-jerk', 'package.json'), 'utf8'));
  assert.equal(pkg.name, '@ai-ecoverse/slicc-jerk');
  assert.equal(pkg.version, '1.2.3');
  assert.deepEqual(pkg.bin, { jerk: 'bin/jerk.js' });
  assert.deepEqual(pkg.slicc, { abi: 'wasi', commands: { jerk: { wasm: 'bin/jerk.wasm' } } });
  assert.deepEqual(pkg.optionalDependencies, { [`@ai-ecoverse/slicc-jerk-${target}`]: '1.2.3' });
});

test('the platform package is pinned to its os and cpu', () => {
  const pkg = JSON.parse(
    readFileSync(join(work, 'npm', `slicc-jerk-${target}`, 'package.json'), 'utf8')
  );
  assert.equal(pkg.name, `@ai-ecoverse/slicc-jerk-${target}`);
  assert.deepEqual(pkg.os, [process.platform]);
  assert.deepEqual(pkg.cpu, [process.arch]);
});

test('the launcher runs the platform binary', () => {
  const modules = join(work, 'install', 'node_modules', '@ai-ecoverse');
  mkdirSync(modules, { recursive: true });
  cpSync(join(work, 'npm', 'slicc-jerk'), join(modules, 'slicc-jerk'), { recursive: true });
  cpSync(join(work, 'npm', `slicc-jerk-${target}`), join(modules, `slicc-jerk-${target}`), {
    recursive: true,
  });
  const launcher = join(modules, 'slicc-jerk', 'bin', 'jerk.js');
  const ok = spawnSync(process.execPath, [launcher, '--version'], { encoding: 'utf8' });
  assert.equal(ok.status, 0, ok.stderr);
  assert.equal(ok.stdout, `jerk ${expectedVersion}\n`);
  const usage = spawnSync(process.execPath, [launcher, '--nope'], { encoding: 'utf8' });
  assert.equal(usage.status, 2);
  rmSync(join(modules, `slicc-jerk-${target}`), { recursive: true, force: true });
  const missing = spawnSync(process.execPath, [launcher, '--version'], { encoding: 'utf8' });
  assert.equal(missing.status, 1);
  assert.match(missing.stderr, new RegExp(`@ai-ecoverse/slicc-jerk-${target} is not installed`));
});

test('staging refuses a missing binary', () => {
  const res = spawnSync(
    process.execPath,
    [
      'scripts/npm-stage.mjs',
      '--version',
      '1.2.3',
      '--from',
      join(work, 'nowhere'),
      '--out',
      join(work, 'bad'),
      '--only',
      target,
    ],
    { cwd: root, encoding: 'utf8' }
  );
  assert.notEqual(res.status, 0);
  assert.match(res.stderr, /missing/);
});
