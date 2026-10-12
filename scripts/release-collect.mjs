#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { assetName, TARGETS } from './targets.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const { values } = parseArgs({
  options: {
    version: { type: 'string' },
    from: { type: 'string', default: join(root, 'artifacts', 'build') },
    out: { type: 'string', default: join(root, 'artifacts', 'release') },
  },
});
if (!values.version) {
  process.stderr.write('usage: release-collect.mjs --version <semver> [--from dir] [--out dir]\n');
  process.exit(2);
}

rmSync(values.out, { recursive: true, force: true });
mkdirSync(values.out, { recursive: true });
const assets = [...TARGETS.map(assetName), 'jerk.wasm'];
const sums = [];
for (const asset of assets) {
  const source = join(values.from, asset, asset);
  if (!existsSync(source)) throw new Error(`missing ${source}`);
  copyFileSync(source, join(values.out, asset));
  sums.push(`${createHash('sha256').update(readFileSync(source)).digest('hex')}  ${asset}`);
}
writeFileSync(join(values.out, 'SHA256SUMS'), `${sums.join('\n')}\n`);
mkdirSync(join(root, 'dist'), { recursive: true });
writeFileSync(join(root, 'dist', 'VERSION'), values.version.replace(/^v/, ''));
process.stdout.write(`collected ${assets.length} assets for ${values.version} in ${values.out}\n`);
