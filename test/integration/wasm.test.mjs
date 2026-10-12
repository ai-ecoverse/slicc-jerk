import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { WASI } from 'node:wasi';
import { expectedVersion, wasm } from './helpers.mjs';

function runWasm(args) {
  const dir = mkdtempSync(join(tmpdir(), 'jerk-wasm-'));
  const out = join(dir, 'out');
  const fs = process.getBuiltinModule('node:fs');
  const fd = fs.openSync(out, 'w');
  const wasi = new WASI({
    version: 'preview1',
    args: ['jerk', ...args],
    stdout: fd,
    stderr: fd,
    returnOnExit: true,
  });
  const instance = new WebAssembly.Instance(
    new WebAssembly.Module(readFileSync(wasm)),
    wasi.getImportObject()
  );
  const code = wasi.start(instance);
  fs.closeSync(fd);
  const text = readFileSync(out, 'utf8');
  rmSync(dir, { recursive: true, force: true });
  return { code, text };
}

test('the kernel build prints its version', {
  skip: !existsSync(wasm) && 'run npm run build:wasm first',
}, () => {
  const res = runWasm(['--version']);
  assert.equal(res.code, 0);
  assert.equal(res.text, `jerk ${expectedVersion}\n`);
});

test('the kernel build has no CLI-dispatch providers', {
  skip: !existsSync(wasm) && 'run npm run build:wasm first',
}, () => {
  const res = runWasm(['list']);
  assert.equal(res.code, 0);
  assert.match(res.text, /^PROVIDER\s+NAME\s+SOURCE\s+CLI\n$/);
});

test('the kernel build keeps the exit codes', {
  skip: !existsSync(wasm) && 'run npm run build:wasm first',
}, () => {
  assert.equal(runWasm(['--nope']).code, 2);
  assert.equal(runWasm(['intercept']).code, 5);
  assert.equal(runWasm(['github', 'get']).code, 2);
});
