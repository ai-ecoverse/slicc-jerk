#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const res = spawnSync('cargo', ['build', '--release', '--target', 'wasm32-wasip1', '-p', 'jerk'], {
  cwd: root,
  stdio: 'inherit',
});
if (res.status !== 0) process.exit(res.status ?? 1);
mkdirSync(join(root, 'bin'), { recursive: true });
copyFileSync(
  join(root, 'target', 'wasm32-wasip1', 'release', 'jerk.wasm'),
  join(root, 'bin', 'jerk.wasm')
);
