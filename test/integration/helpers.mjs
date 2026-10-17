import { spawn, spawnSync } from 'node:child_process';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { after } from 'node:test';
import { fileURLToPath } from 'node:url';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
export const exe = process.platform === 'win32' ? 'jerk.exe' : 'jerk';
export const binary = process.env.JERK_BIN || join(root, 'target', 'debug', exe);
export const wasm = join(root, 'bin', 'jerk.wasm');
export const expectedVersion = process.env.JERK_VERSION || '0.0.0';

export function sandbox() {
  const dir = mkdtempSync(join(tmpdir(), 'jerk-it-'));
  after(() => rmSync(dir, { recursive: true, force: true }));
  const bin = join(dir, 'bin');
  mkdirSync(bin);
  const env = {
    PATH: bin,
    SystemRoot: process.env.SystemRoot || '',
    HOME: join(dir, 'home'),
    USERPROFILE: join(dir, 'home'),
    XDG_CONFIG_HOME: join(dir, 'config'),
    XDG_STATE_HOME: join(dir, 'state'),
    APPDATA: join(dir, 'config'),
    LOCALAPPDATA: join(dir, 'state'),
    SLICC_JERK_SYSTEM_DIR: join(dir, 'system'),
  };
  return {
    dir,
    env,
    userFile: join(dir, 'config', 'slicc-jerk', 'providers.toml'),
    stateDir:
      process.platform === 'darwin'
        ? join(dir, 'home', 'Library', 'Application Support', 'slicc-jerk')
        : join(dir, 'state', 'slicc-jerk'),
    fakeCli(name) {
      const file = join(bin, process.platform === 'win32' ? `${name}.exe` : name);
      writeFileSync(file, '');
      return file;
    },
    script(name, body) {
      const js = join(bin, `${name}.js`);
      writeFileSync(
        js,
        `const fs = require('node:fs');\nconst args = process.argv.slice(2);\nfs.appendFileSync(${JSON.stringify(join(dir, 'calls.jsonl'))}, JSON.stringify({ name: ${JSON.stringify(name)}, args, env: Object.keys(process.env).filter((k) => k.endsWith('_TOKEN')) }) + '\\n');\n${body}\n`
      );
      if (process.platform === 'win32') {
        writeFileSync(join(bin, `${name}.cmd`), `@"${process.execPath}" "%~dp0${name}.js" %*\r\n`);
      } else {
        const file = join(bin, name);
        writeFileSync(file, `#!${process.execPath}\nrequire(${JSON.stringify(js)});\n`);
        chmodSync(file, 0o755);
      }
    },
    calls() {
      const file = join(dir, 'calls.jsonl');
      if (!existsSync(file)) return [];
      return readFileSync(file, 'utf8')
        .trim()
        .split('\n')
        .map((line) => JSON.parse(line));
    },
    writeUser(text) {
      mkdirSync(join(dir, 'config', 'slicc-jerk'), { recursive: true });
      writeFileSync(join(dir, 'config', 'slicc-jerk', 'providers.toml'), text);
    },
    jerk(args, extra = {}) {
      return jerk(args, { env: { ...env, ...extra } });
    },
    jerkAsync(args, extra = {}, input = undefined) {
      return jerkAsync(args, { env: { ...env, ...extra }, input });
    },
  };
}

export function jerk(args, options = {}) {
  if (!existsSync(binary)) throw new Error(`build jerk first: ${binary} is missing`);
  return spawnSync(binary, args, { encoding: 'utf8', ...options });
}

export function jerkAsync(args, { input, ...options } = {}) {
  if (!existsSync(binary)) throw new Error(`build jerk first: ${binary} is missing`);
  return new Promise((resolve) => {
    const child = spawn(binary, args, { ...options });
    if (input !== undefined) child.stdin.end(input);
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
    child.on('error', (err) => {
      resolve({ status: null, signal: null, error: err, stdout, stderr });
    });
    child.on('close', (status, signal) => {
      resolve({ status, signal, stdout, stderr });
    });
  });
}
