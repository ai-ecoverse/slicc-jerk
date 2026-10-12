#!/usr/bin/env node
import { chmodSync, copyFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { assetName, NAME, SCOPE, TARGETS } from './targets.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const common = {
  license: 'Apache-2.0',
  repository: { type: 'git', url: 'git+https://github.com/ai-ecoverse/slicc-jerk.git' },
  homepage: 'https://github.com/ai-ecoverse/slicc-jerk#readme',
};

const { values } = parseArgs({
  options: {
    version: { type: 'string' },
    from: { type: 'string', default: join(root, 'artifacts', 'release') },
    out: { type: 'string', default: join(root, 'dist', 'npm') },
    only: { type: 'string', multiple: true },
  },
});

if (!values.version) {
  process.stderr.write(
    'usage: npm-stage.mjs --version <semver> [--from dir] [--out dir] [--only os-arch]...\n'
  );
  process.exit(2);
}
const version = values.version.replace(/^v/, '');
const selected = values.only?.length ? TARGETS.filter((t) => values.only.includes(t.id)) : TARGETS;

function write(dir, file, content) {
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    join(dir, file),
    typeof content === 'string' ? content : `${JSON.stringify(content, null, 2)}\n`
  );
}

function need(file) {
  if (!existsSync(file)) throw new Error(`missing ${file}`);
  return file;
}

function stagePlatform(target) {
  const name = `${NAME}-${target.id}`;
  const dir = join(values.out, `slicc-jerk-${target.id}`);
  const file = target.os === 'win32' ? 'jerk.exe' : 'jerk';
  mkdirSync(join(dir, 'bin'), { recursive: true });
  copyFileSync(need(join(values.from, assetName(target))), join(dir, 'bin', file));
  chmodSync(join(dir, 'bin', file), 0o755);
  copyFileSync(join(root, 'LICENSE'), join(dir, 'LICENSE'));
  write(dir, 'package.json', {
    name,
    version,
    description: `The jerk binary for ${target.label}`,
    ...common,
    os: [target.os],
    cpu: [target.cpu],
    files: [`bin/${file}`],
    preferUnplugged: true,
  });
  write(
    dir,
    'README.md',
    `# ${name}\n\nThe \`jerk\` binary for ${target.label}. Install [${NAME}](https://www.npmjs.com/package/${NAME}) instead of this package.\n`
  );
  return name;
}

function stageMain(platforms) {
  const dir = join(values.out, 'slicc-jerk');
  mkdirSync(join(dir, 'bin'), { recursive: true });
  copyFileSync(join(root, 'npm', 'jerk.js'), join(dir, 'bin', 'jerk.js'));
  chmodSync(join(dir, 'bin', 'jerk.js'), 0o755);
  copyFileSync(need(join(values.from, 'jerk.wasm')), join(dir, 'bin', 'jerk.wasm'));
  copyFileSync(join(root, 'LICENSE'), join(dir, 'LICENSE'));
  copyFileSync(join(root, 'README.md'), join(dir, 'README.md'));
  write(dir, 'package.json', {
    name: NAME,
    version,
    description:
      "Get, list, create and drop OAuth tokens across operating systems and inside SLICC, by dispatching to each provider's own CLI",
    ...common,
    bin: { jerk: 'bin/jerk.js' },
    files: ['bin/jerk.js', 'bin/jerk.wasm'],
    engines: { node: '>=18' },
    slicc: { abi: 'wasi', commands: { jerk: { wasm: 'bin/jerk.wasm' } } },
    optionalDependencies: Object.fromEntries(platforms.map((name) => [name, version])),
  });
}

function stageOAuthWasm() {
  const oauthName = `${SCOPE}/jerk-oauth`;
  const src = join(root, 'dist', 'jerk-oauth');
  if (!existsSync(join(src, 'jerk_oauth_wasm.js'))) return;
  const dir = join(values.out, 'jerk-oauth');
  mkdirSync(dir, { recursive: true });
  for (const file of [
    'jerk_oauth_wasm.js',
    'jerk_oauth_wasm.d.ts',
    'jerk_oauth_wasm_bg.wasm',
    'jerk_oauth_wasm_bg.wasm.d.ts',
  ]) {
    if (existsSync(join(src, file))) copyFileSync(join(src, file), join(dir, file));
  }
  copyFileSync(join(root, 'LICENSE'), join(dir, 'LICENSE'));
  write(dir, 'package.json', {
    name: oauthName,
    version,
    description: 'Pure OAuth helpers for jerk, compiled to WebAssembly (ESM, web target)',
    ...common,
    type: 'module',
    main: 'jerk_oauth_wasm.js',
    types: 'jerk_oauth_wasm.d.ts',
    files: [
      'jerk_oauth_wasm.js',
      'jerk_oauth_wasm.d.ts',
      'jerk_oauth_wasm_bg.wasm',
      'jerk_oauth_wasm_bg.wasm.d.ts',
    ],
    sideEffects: false,
  });
  write(
    dir,
    'README.md',
    `# ${oauthName}\n\nPure OAuth helpers (PKCE, relay state, capture, exchange, refresh, scopes and backoff) compiled from jerk-oauth to WebAssembly. Used by bios (route (b) sign-ins) and the vault (grant refresh). See [slicc-jerk](https://github.com/ai-ecoverse/slicc-jerk) for details.\n`
  );
}

rmSync(values.out, { recursive: true, force: true });
stageMain(selected.map(stagePlatform));
stageOAuthWasm();
process.stdout.write(
  `staged ${NAME}@${version} and ${selected.length} platform packages in ${values.out}\n`
);
