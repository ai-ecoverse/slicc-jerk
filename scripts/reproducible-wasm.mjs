#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'dist', 'jerk-oauth');
const work = mkdtempSync(join(tmpdir(), 'jerk-reproducible-'));

function build(side) {
  const target = join(work, `target-${side}`);
  const res = spawnSync(process.execPath, [join(root, 'scripts', 'build-jerk-oauth-wasm.mjs')], {
    cwd: root,
    stdio: 'inherit',
    env: { ...process.env, CARGO_TARGET_DIR: target },
  });
  if (res.status !== 0) process.exit(res.status ?? 1);
  cpSync(out, join(work, side), { recursive: true });
  rmSync(target, { recursive: true, force: true });
}

build('a');
build('b');
const files = readdirSync(join(work, 'a')).sort();
const differ = files.filter(
  (file) => !readFileSync(join(work, 'a', file)).equals(readFileSync(join(work, 'b', file)))
);
const extra = readdirSync(join(work, 'b')).filter((file) => !files.includes(file));
rmSync(work, { recursive: true, force: true });
if (differ.length || extra.length || files.length === 0) {
  process.stderr.write(
    `reproducible-wasm: FAIL ${[...differ, ...extra].join(', ') || 'no files'} differ between two builds\n`
  );
  process.exit(1);
}
process.stdout.write(
  `reproducible-wasm: PASS ${files.length} jerk-oauth files byte-identical across two fresh target dirs\n`
);
