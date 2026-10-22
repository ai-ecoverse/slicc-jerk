import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { assetName, NAME, SCOPE, TARGETS } from '../../scripts/targets.mjs';

const read = (path) => readFile(new URL(`../../${path}`, import.meta.url), 'utf8');
const ci = await read('.github/workflows/ci.yml');
const pkg = JSON.parse(await read('package.json'));
const release = JSON.parse(await read('.releaserc.json'));
const pipeline = ci.split('\n  released:\n')[0];
const input = (name) => pipeline.match(new RegExp(`\\n {6}${name}: (.+)\\n`))?.[1];
const job = (name) => ci.match(new RegExp(`\\n {2}${name}:\\n((?: {4}.*\\n|\\n)+)`))?.[1] ?? '';

test('ci.yml is the one workflow, and the only caller of pipeline.yml at the devDependency version', async () => {
  assert.deepEqual(await readdir(new URL('../../.github/', import.meta.url)), ['workflows']);
  assert.deepEqual(await readdir(new URL('../../.github/workflows/', import.meta.url)), ['ci.yml']);
  const version = pkg.devDependencies['@ai-ecoverse/slicc-shared-web'];
  assert.match(version, /^2\.\d+\.\d+$/);
  assert.ok(
    ci.includes(
      `\njobs:\n  ci:\n    uses: ai-ecoverse/slicc-shared-web/.github/workflows/pipeline.yml@v${version}\n    secrets: inherit\n    with:\n`
    )
  );
  assert.equal(ci.match(/slicc-shared-web\//g).length, 1);
  assert.deepEqual(ci.split('\njobs:\n')[1].match(/^ {2}[a-z-]+:$/gm), [
    '  ci:',
    '  released:',
    '  binaries:',
    '  assets:',
  ]);
});

test('CI runs on pull requests, the merge queue and main, cancelling only superseded pull request runs', () => {
  assert.ok(
    ci.includes('\non:\n  pull_request:\n  merge_group:\n  push:\n    branches: [main]\n\n')
  );
  assert.ok(
    ci.includes(
      "\nconcurrency:\n  group: ci-${{ github.ref }}\n  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n"
    )
  );
  assert.doesNotMatch(ci, /workflow_dispatch|schedule:|workflow_run|workflow_call/);
  assert.doesNotMatch(ci, /vars\.|CERT_/);
  const top = ci.split('\njobs:\n')[0];
  for (const permission of [
    'contents: write',
    'actions: read',
    'issues: write',
    'pull-requests: write',
    'statuses: write',
    'id-token: write',
    'attestations: write',
  ])
    assert.ok(top.includes(`\n  ${permission}\n`), permission);
});

test('the pipeline lints, checks the wasm build, and shards the node suite', () => {
  assert.equal(input('runs-on'), undefined);
  assert.match(input('lint'), /npm run lint && npm run lint:wasm$/);
  assert.equal(pkg.scripts['lint:wasm'], 'node scripts/reproducible-wasm.mjs');
  assert.match(input('test'), /npm run test:ci$/);
  assert.match(pkg.scripts['test:ci'], /build:wasm && npm run build:oauth-wasm && npm test$/);
  assert.equal(input('test-glob'), 'test/integration/*.test.mjs');
  assert.ok(pkg.scripts.test.endsWith(` ${input('test-glob')}`), pkg.scripts.test);
});

test('semantic-release only tags and drafts the release, and npm trusts ci.yml', () => {
  assert.deepEqual(release.branches, ['main']);
  assert.deepEqual(release.plugins, [
    '@semantic-release/commit-analyzer',
    '@semantic-release/release-notes-generator',
    ['@semantic-release/github', { draftRelease: true }],
  ]);
  assert.equal(pkg.fledgling.workflow, 'ci.yml');
  assert.equal(pkg.fledgling.repo, 'ai-ecoverse/slicc-jerk');
  assert.deepEqual(pkg.fledgling.include, [
    NAME,
    ...TARGETS.map((target) => `${NAME}-${target.id}`),
    `${SCOPE}/jerk-oauth`,
  ]);
});

test('the binaries and assets follow only a new tag on main, and ship every asset install.sh expects', () => {
  assert.match(job('released'), /needs: ci\n/);
  assert.match(
    job('released'),
    /if: github\.event_name == 'push' && github\.ref == 'refs\/heads\/main'\n/
  );
  assert.match(
    job('binaries'),
    /needs: released\n {4}if: needs\.released\.outputs\.version != ''\n/
  );
  assert.match(job('assets'), /needs: \[released, binaries\]\n/);
  const built = [...job('binaries').matchAll(/:(jerk[\w.-]*)(?=[ '])/g)].map((m) => m[1]);
  assert.deepEqual(built.sort(), [...TARGETS.map(assetName), 'jerk.wasm'].sort());
  for (const target of TARGETS) assert.ok(job('binaries').includes(`${target.rust}:`), target.rust);
  assert.match(
    job('binaries'),
    /scripts\/sign-macos\.sh out\/jerk-darwin-arm64 out\/jerk-darwin-amd64/
  );
  assert.match(job('binaries'), /Authority=Developer ID Application/);
  assert.match(job('binaries'), /--version\)" = "jerk \$JERK_VERSION"/);
  assert.match(job('binaries'), /grep -qi 'static'/);
  const assets = job('assets');
  const order = [
    'npm run build:oauth-wasm',
    'scripts/release-collect.mjs',
    'scripts/npm-stage.mjs',
    'subject-checksums: artifacts/release/SHA256SUMS',
    'gh release upload "v$VERSION" jerk-* jerk.wasm SHA256SUMS#SHA256SUMS "../../install.sh#install.sh"',
    'node scripts/npm-publish.mjs\n',
    'gh release edit "v$VERSION" --draft=false --latest',
    'releases/latest/download/install.sh',
  ].map((step) => assets.indexOf(step));
  assert.ok(
    order.every((at) => at >= 0),
    JSON.stringify(order)
  );
  assert.deepEqual(
    order,
    [...order].sort((a, b) => a - b)
  );
  assert.match(assets, /id-token: write\n {6}attestations: write\n/);
});
