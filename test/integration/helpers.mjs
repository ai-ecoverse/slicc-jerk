import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
export const exe = process.platform === 'win32' ? 'jerk.exe' : 'jerk';
export const binary = process.env.JERK_BIN || join(root, 'target', 'debug', exe);
export const wasm = join(root, 'bin', 'jerk.wasm');
export const expectedVersion = process.env.JERK_VERSION || '0.0.0';

export function jerk(args, options = {}) {
  if (!existsSync(binary)) throw new Error(`build jerk first: ${binary} is missing`);
  return spawnSync(binary, args, { encoding: 'utf8', ...options });
}
