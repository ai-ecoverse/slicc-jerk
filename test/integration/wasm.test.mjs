import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { WASI } from 'node:wasi';
import { expectedVersion, wasm } from './helpers.mjs';

function runWasm(args, { env, preopens } = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'jerk-wasm-'));
  const out = join(dir, 'out');
  const fs = process.getBuiltinModule('node:fs');
  const fd = fs.openSync(out, 'w');
  const wasi = new WASI({
    version: 'preview1',
    args: ['jerk', ...args],
    env,
    preopens,
    stdout: fd,
    stderr: fd,
    returnOnExit: true,
  });
  const module = new WebAssembly.Module(readFileSync(wasm));
  const imports = wasi.getImportObject();
  for (const { module: from, name } of WebAssembly.Module.imports(module)) {
    if (from !== 'wasix_32v1') continue;
    imports[from] ??= {};
    imports[from][name] = () => 52;
  }
  const instance = new WebAssembly.Instance(module, imports);
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

test('the kernel get accepts --reveal and still prints only the placeholder', {
  skip: !existsSync(wasm) && 'run npm run build:wasm first',
}, () => {
  const cfg = mkdtempSync(join(tmpdir(), 'jerk-wasm-cfg-'));
  writeFileSync(
    join(cfg, 'providers.toml'),
    '[providers.demo]\nname = "Demo"\nenv = ["DEMO_TOKEN"]\nrequires = "demo"\nget = { run = ["demo", "token"] }\n'
  );
  const opts = {
    env: { SLICC_JERK_CONFIG: '/cfg/providers.toml', DEMO_TOKEN: 'sealed:demo:placeholder' },
    preopens: { '/cfg': cfg },
  };
  try {
    const plain = runWasm(['demo', 'get', '--reveal'], opts);
    assert.equal(plain.code, 0);
    assert.equal(plain.text, 'sealed:demo:placeholder\n');
    const json = runWasm(['demo', 'get', '--json'], opts);
    assert.equal(json.code, 0);
    const both = runWasm(['demo', 'get', '--json', '--reveal'], opts);
    assert.equal(both.code, 0);
    assert.equal(both.text, json.text);
    assert.deepEqual(JSON.parse(both.text), {
      provider: 'demo',
      source: 'env',
      placeholder: 'sealed:demo:placeholder',
    });
    const bogus = runWasm(['demo', 'get', '--bogus'], opts);
    assert.equal(bogus.code, 2);
    assert.match(bogus.text, /unexpected --bogus/);
  } finally {
    rmSync(cfg, { recursive: true, force: true });
  }
});

test('the kernel build needs the kernel browser for jerk tab', {
  skip: !existsSync(wasm) && 'run npm run build:wasm first',
}, () => {
  const res = runWasm(['tab', 'list']);
  assert.equal(res.code, 4);
  assert.equal(res.text, 'jerk tab: this kernel offers no browser: SLICC_CDP_URL is not set\n');
});
