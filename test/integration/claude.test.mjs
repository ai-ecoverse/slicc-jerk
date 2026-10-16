import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const TOKEN = `sk-ant-oat01-${'Ab3_-'.repeat(19)}Zq9x`;
const OTHER = `sk-ant-oat01-${'Cd4_-'.repeat(19)}Wv7y`;
const ESC = '\u001b';

function noisy(token) {
  const cut = 50;
  return [
    `${ESC}[?25l${ESC}]8;;https://claude.ai/oauth${ESC}\\Opening browser to sign in…${ESC}]8;;${ESC}\\\r\n`,
    `${ESC}[2K${ESC}[1A${ESC}[2K${ESC}[G⠋ Processing authentication…\r\n`,
    `${ESC}[2K${ESC}[1A${ESC}[2K${ESC}[G${ESC}[32m✓ Long-lived authentication token created successfully!${ESC}[39m\r\n`,
    '\r\n',
    'Your OAuth token (valid for 1 year):\r\n',
    '\r\n',
    `${ESC}[33m${token.slice(0, cut)}\r\n${token.slice(cut)}${ESC}[39m\r\n`,
    '\r\n',
    `${ESC}[2mStore this token securely. You won't be able to see it again.${ESC}[22m\r\n`,
    '\r\n',
    `${ESC}[2mUse this token by setting: export CLAUDE_CODE_OAUTH_TOKEN=<token>${ESC}[22m\r\n`,
    `${ESC}[?25h`,
  ].join('');
}

function claude(sb, { version = '2.1.300 (Claude Code)', token = TOKEN, setupExit = 0 } = {}) {
  sb.script(
    'claude',
    `if (args[0] === '--version') { process.stdout.write(${JSON.stringify(`${version}\n`)}); process.exit(0); }
if (args[0] === 'setup-token') {
  if (args.length !== 1) { process.stderr.write('error: too many arguments\\n'); process.exit(1); }
  process.stdout.write(${JSON.stringify(token ? noisy(token) : 'Welcome to Claude Code\n')});
  process.exitCode = ${setupExit};
} else
if (args[0] === 'auth' && args[1] === 'status' && args[2] === '--json') {
  process.stdout.write(JSON.stringify({ loggedIn: true, authMethod: 'claude.ai', apiProvider: 'firstParty', email: 'dev@example.com', subscriptionType: 'max' }, null, 2) + '\\n');
} else {
  process.stderr.write('unexpected claude call\\n');
  process.exitCode = 9;
}`
  );
}

const FILE = { SLICC_JERK_STORE: 'file' };

function claudeRuns(sb) {
  return sb
    .calls()
    .filter((c) => c.name === 'claude')
    .map((c) => c.args);
}

test('create keeps the token setup-token prints through ANSI noise and line wraps', () => {
  const sb = sandbox();
  claude(sb);
  const created = sb.jerk(['claude', 'create'], FILE);
  assert.equal(created.status, 0, created.stderr);
  assert.equal(created.stdout, 'sk-a…Zq9x\n');
  assert.equal(created.stderr, 'jerk: claude: created token (no id), kept for get\n');
  assert.deepEqual(claudeRuns(sb), [['setup-token']]);

  const revealed = sb.jerk(['claude', 'get', '--reveal'], FILE);
  assert.equal(revealed.status, 0, revealed.stderr);
  assert.equal(revealed.stdout, `${TOKEN}\n`);

  const piped = sb.jerk(['claude', 'get'], FILE);
  assert.equal(piped.status, 0, piped.stderr);
  assert.equal(piped.stdout, 'sk-a…Zq9x\n');
  assert.doesNotMatch(piped.stdout + piped.stderr, new RegExp(TOKEN.slice(13, 40)));

  const json = JSON.parse(sb.jerk(['claude', 'get', '--json'], FILE).stdout);
  assert.equal(json.source, 'kept');
  assert.equal(json.masked, 'sk-a…Zq9x');
  assert.equal(json.token, undefined);
  assert.match(json.expires_at, /^\d{4}-\d\d-\d\dT/);
  assert.deepEqual(claudeRuns(sb), [['setup-token']]);
});

test('create again is a no-op until --force mints and keeps a new token', () => {
  const sb = sandbox();
  claude(sb);
  assert.equal(sb.jerk(['claude', 'create'], FILE).status, 0);
  const again = sb.jerk(['claude', 'create'], FILE);
  assert.equal(again.status, 0, again.stderr);
  assert.equal(again.stdout, 'claude: already signed in; use --force to sign in again\n');
  assert.deepEqual(claudeRuns(sb), [['setup-token']]);

  claude(sb, { token: OTHER });
  const forced = sb.jerk(['claude', 'create', '--force', '--reveal'], FILE);
  assert.equal(forced.status, 0, forced.stderr);
  assert.equal(forced.stdout, `${OTHER}\n`);
  assert.equal(sb.jerk(['claude', 'get', '--reveal'], FILE).stdout, `${OTHER}\n`);
});

test('ANTHROPIC_API_KEY wins over the kept token without running claude', () => {
  const sb = sandbox();
  claude(sb);
  assert.equal(sb.jerk(['claude', 'create'], FILE).status, 0);
  const res = sb.jerk(['claude', 'get', '--json', '--reveal'], {
    ...FILE,
    ANTHROPIC_API_KEY: 'sk-ant-api03-envkeyenvkeyenvkey',
  });
  assert.equal(res.status, 0, res.stderr);
  const doc = JSON.parse(res.stdout);
  assert.equal(doc.source, 'env');
  assert.equal(doc.token, 'sk-ant-api03-envkeyenvkeyenvkey');
  assert.deepEqual(claudeRuns(sb), [['setup-token']]);
});

test('drop forgets the kept token and leaves the Claude Code sign-in alone', () => {
  const sb = sandbox();
  claude(sb);
  assert.equal(sb.jerk(['claude', 'create'], FILE).status, 0);
  const refused = sb.jerk(['claude', 'drop'], FILE);
  assert.equal(refused.status, 2);
  assert.match(refused.stderr, /pass --yes/);

  const dropped = sb.jerk(['claude', 'drop', '--yes'], FILE);
  assert.equal(dropped.status, 0, dropped.stderr);
  assert.equal(dropped.stdout, 'claude: dropped\n');

  const gone = sb.jerk(['claude', 'get'], FILE);
  assert.equal(gone.status, 3);
  assert.match(gone.stderr, /jerk: claude: no kept token/);
  assert.match(gone.stderr, /sign in with jerk claude create/);
  assert.deepEqual(claudeRuns(sb), [['setup-token']]);
});

test('get needs no claude on PATH and asks for create when nothing is kept', () => {
  const sb = sandbox();
  const res = sb.jerk(['claude', 'get'], FILE);
  assert.equal(res.status, 3);
  assert.match(res.stderr, /no kept token/);
});

test('create fails without keeping anything when setup-token prints no token or fails', () => {
  const sb = sandbox();
  claude(sb, { token: '' });
  const empty = sb.jerk(['claude', 'create'], FILE);
  assert.equal(empty.status, 1);
  assert.match(empty.stderr, /jerk: claude: create printed no token/);
  assert.equal(sb.jerk(['claude', 'get'], FILE).status, 3);

  claude(sb, { setupExit: 1 });
  const failed = sb.jerk(['claude', 'create'], FILE);
  assert.equal(failed.status, 1);
  assert.match(failed.stderr, /claude create exited 1/);
  assert.doesNotMatch(failed.stdout + failed.stderr, new RegExp(TOKEN.slice(13, 40)));
  assert.equal(sb.jerk(['claude', 'get'], FILE).status, 3);
});

test('list shows claude auth status and a too-old CLI exits 4', () => {
  const sb = sandbox();
  claude(sb);
  const listed = sb.jerk(['claude', 'list', '--json'], FILE);
  assert.equal(listed.status, 0, listed.stderr);
  assert.deepEqual(JSON.parse(listed.stdout).items[0].email, 'dev@example.com');
  assert.deepEqual(claudeRuns(sb), [['--version'], ['auth', 'status', '--json']]);

  claude(sb, { version: '2.1.40 (Claude Code)' });
  const old = sb.jerk(['claude', 'list'], FILE);
  assert.equal(old.status, 4);
  assert.match(old.stderr, /claude 2\.1\.40 is too old; this needs 2\.1\.41 or newer/);
});

test('config show claude and list --json describe the provider', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'claude']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /parse = "scan:sk-ant-oat"/);
  assert.match(shown.stdout, /keep = true/);
  const entry = JSON.parse(sb.jerk(['list', '--json']).stdout).find((p) => p.provider === 'claude');
  assert.deepEqual(entry.env, ['ANTHROPIC_API_KEY']);
  assert.equal(entry.requires, 'claude');
  assert.equal(entry.cli, false);
});
