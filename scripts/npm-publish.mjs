#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const { values } = parseArgs({
  options: {
    dir: { type: 'string', default: join(root, 'dist', 'npm') },
    tag: { type: 'string', default: 'latest' },
    'dry-run': { type: 'boolean', default: false },
  },
});
const dryRun = values['dry-run'];
const gate = 'SLICC_NPM_PUBLISH';

function fail(message) {
  process.stderr.write(`npm-publish: ${message}\n`);
  process.exit(1);
}

function npm(args, options = {}) {
  return spawnSync('npm', args, {
    encoding: 'utf8',
    shell: process.platform === 'win32',
    ...options,
  });
}

function published(name, version) {
  const res = npm(['view', `${name}@${version}`, 'version']);
  return res.status === 0 && res.stdout.trim() === version;
}

const packages = readdirSync(values.dir)
  .map((entry) => join(values.dir, entry))
  .map((dir) => ({ dir, pkg: JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')) }))
  .sort(
    (a, b) =>
      Number(Boolean(a.pkg.optionalDependencies)) - Number(Boolean(b.pkg.optionalDependencies))
  );

if (!dryRun && process.env[gate] !== 'true') {
  process.stdout.write(
    `::notice::skipping npm; set the repository variable ${gate} to true to publish\n`
  );
  process.exit(0);
}

for (const { dir, pkg } of packages) {
  if (!dryRun && published(pkg.name, pkg.version)) {
    process.stdout.write(`${pkg.name}@${pkg.version} is already published\n`);
    continue;
  }
  const tag =
    dryRun && values.tag === 'latest' && pkg.version.includes('-') ? 'dry-run' : values.tag;
  const args = [
    'publish',
    dir,
    '--access',
    'public',
    '--tag',
    tag,
    dryRun ? '--dry-run' : '--provenance',
  ];
  const res = npm(args, { stdio: 'inherit' });
  if (res.status !== 0) fail(`npm publish ${pkg.name}@${pkg.version} failed`);
}
