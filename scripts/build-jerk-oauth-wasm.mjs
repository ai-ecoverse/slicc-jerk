#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { mkdirSync, rmSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'dist', 'jerk-oauth');
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const cargoHome = process.env.CARGO_HOME
  ? resolve(process.env.CARGO_HOME)
  : join(homedir(), '.cargo');
const remap = [
  `--remap-path-prefix=${cargoHome}=/cargo`,
  `--remap-path-prefix=${root}=/build`,
].join(' ');
const rustflags = [process.env.RUSTFLAGS, remap].filter(Boolean).join(' ');
const cargo = spawnSync(
  'cargo',
  ['build', '--release', '--locked', '--target', 'wasm32-unknown-unknown', '-p', 'jerk-oauth-wasm'],
  { cwd: root, stdio: 'inherit', env: { ...process.env, RUSTFLAGS: rustflags } }
);
if (cargo.status !== 0) process.exit(cargo.status ?? 1);

const targetDir = process.env.CARGO_TARGET_DIR
  ? resolve(process.env.CARGO_TARGET_DIR)
  : join(root, 'target');
const wasm = join(targetDir, 'wasm32-unknown-unknown', 'release', 'jerk_oauth_wasm.wasm');
const bindgen = spawnSync(
  'wasm-bindgen',
  [wasm, '--out-dir', out, '--target', 'web', '--omit-default-module-path'],
  { cwd: root, stdio: 'inherit' }
);
if (bindgen.status !== 0) process.exit(bindgen.status ?? 1);
