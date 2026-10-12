#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { mkdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'dist', 'jerk-oauth');
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const cargo = spawnSync(
  'cargo',
  ['build', '--release', '--target', 'wasm32-unknown-unknown', '-p', 'jerk-oauth-wasm'],
  { cwd: root, stdio: 'inherit' }
);
if (cargo.status !== 0) process.exit(cargo.status ?? 1);

const wasm = join(root, 'target', 'wasm32-unknown-unknown', 'release', 'jerk_oauth_wasm.wasm');
const bindgen = spawnSync(
  'wasm-bindgen',
  [wasm, '--out-dir', out, '--target', 'web', '--omit-default-module-path'],
  { cwd: root, stdio: 'inherit' }
);
if (bindgen.status !== 0) process.exit(bindgen.status ?? 1);
