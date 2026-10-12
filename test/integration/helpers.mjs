import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
export const exe = process.platform === 'win32' ? 'jerk.exe' : 'jerk';
export const binary = process.env.JERK_BIN || join(root, 'target', 'debug', exe);
export const wasm = join(root, 'bin', 'jerk.wasm');
export const expectedVersion = process.env.JERK_VERSION || '0.0.0';

export function sandbox() {
  const dir = mkdtempSync(join(tmpdir(), 'jerk-it-'));
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
    fakeCli(name) {
      const file = join(bin, process.platform === 'win32' ? `${name}.exe` : name);
      writeFileSync(file, '');
      return file;
    },
    writeUser(text) {
      mkdirSync(join(dir, 'config', 'slicc-jerk'), { recursive: true });
      writeFileSync(join(dir, 'config', 'slicc-jerk', 'providers.toml'), text);
    },
    jerk(args) {
      return jerk(args, { env });
    },
  };
}

export function jerk(args, options = {}) {
  if (!existsSync(binary)) throw new Error(`build jerk first: ${binary} is missing`);
  return spawnSync(binary, args, { encoding: 'utf8', ...options });
}
