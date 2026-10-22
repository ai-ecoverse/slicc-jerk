#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'dist', 'jerk-oauth');
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const hosts = {
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'linux-arm64': 'aarch64-unknown-linux-musl',
  'linux-x64': 'x86_64-unknown-linux-musl',
  'win32-x64': 'x86_64-pc-windows-msvc',
};

function lockedVersion() {
  const lock = readFileSync(join(root, 'Cargo.lock'), 'utf8');
  const version = /\[\[package\]\]\nname = "wasm-bindgen"\nversion = "([^"]+)"/.exec(lock)?.[1];
  if (!version) throw new Error('no wasm-bindgen in Cargo.lock');
  return version;
}

async function download(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: ${res.status}`);
  return Buffer.from(await res.arrayBuffer());
}

async function wasmBindgen() {
  const version = lockedVersion();
  const found = spawnSync('wasm-bindgen', ['--version'], { encoding: 'utf8' });
  if (found.status === 0 && found.stdout.trim() === `wasm-bindgen ${version}`)
    return 'wasm-bindgen';
  const host = hosts[`${process.platform}-${process.arch}`];
  if (!host) throw new Error(`install wasm-bindgen ${version}: no prebuilt one for this host`);
  const name = `wasm-bindgen-${version}-${host}`;
  const tools = join(root, 'target', 'tools');
  const bin = join(tools, name, process.platform === 'win32' ? 'wasm-bindgen.exe' : 'wasm-bindgen');
  if (existsSync(bin)) return bin;
  const base = `https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${version}/${name}.tar.gz`;
  const tarball = await download(base);
  const expected = (await download(`${base}.sha256sum`)).toString('utf8').split(/\s+/)[0];
  const actual = createHash('sha256').update(tarball).digest('hex');
  if (actual !== expected)
    throw new Error(`${name}.tar.gz: sha256 ${actual}, expected ${expected}`);
  mkdirSync(tools, { recursive: true });
  const file = join(tools, `${name}.tar.gz`);
  writeFileSync(file, tarball);
  const tar = spawnSync('tar', ['-xzf', file, '-C', tools], { stdio: 'inherit' });
  if (tar.status !== 0 || !existsSync(bin)) throw new Error(`could not unpack ${file}`);
  return bin;
}

const bindgenBin = await wasmBindgen();

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
  bindgenBin,
  [wasm, '--out-dir', out, '--target', 'web', '--omit-default-module-path'],
  { cwd: root, stdio: 'inherit' }
);
if (bindgen.status !== 0) process.exit(bindgen.status ?? 1);
