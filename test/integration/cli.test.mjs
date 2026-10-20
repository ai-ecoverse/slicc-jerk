import assert from 'node:assert/strict';
import { test } from 'node:test';
import { expectedVersion, sandbox } from './helpers.mjs';

test('--version prints the version', () => {
  const sb = sandbox();
  for (const flag of ['--version', '-V']) {
    const res = sb.jerk([flag]);
    assert.equal(res.status, 0);
    assert.equal(res.stdout, `jerk ${expectedVersion}\n`);
    assert.equal(res.stderr, '');
  }
});

test('--help and no arguments print usage', () => {
  const sb = sandbox();
  for (const args of [[], ['--help'], ['-h']]) {
    const res = sb.jerk(args);
    assert.equal(res.status, 0);
    assert.match(res.stdout, /^jerk: get, list, create and drop OAuth tokens/);
    assert.match(res.stdout, /--config <file>/);
    assert.match(
      res.stdout,
      /Exit codes: 0 ok, 1 error, 2 usage, 3 sign-in needed, 4 not available here, 5 unsupported/
    );
  }
});

test('unknown options, commands and providers exit 2', () => {
  const sb = sandbox();
  assert.equal(sb.jerk(['--nope']).status, 2);
  assert.match(sb.jerk(['github', 'steal']).stderr, /unknown command steal/);
  assert.equal(sb.jerk(['github']).status, 2);
  const unknown = sb.jerk(['nope', 'get']);
  assert.equal(unknown.status, 2);
  assert.match(unknown.stderr, /unknown provider nope/);
});

test('jerk list shows the built-in providers and finds their CLIs on PATH', () => {
  const sb = sandbox();
  sb.fakeCli('gh');
  const res = sb.jerk(['list']);
  assert.equal(res.status, 0, res.stderr);
  const lines = res.stdout.trimEnd().split('\n');
  assert.match(lines[0], /^PROVIDER\s+NAME\s+SOURCE\s+CLI$/);
  assert.ok(
    lines.some((l) => /^github\s+GitHub \(gh\)\s+built-in\s+gh \(found\)$/.test(l)),
    res.stdout
  );
  assert.ok(
    lines.some((l) => /^npm\s+npm registry\s+built-in\s+npm \(missing\)$/.test(l)),
    res.stdout
  );
  const json = JSON.parse(sb.jerk(['list', '--json']).stdout);
  assert.deepEqual(
    json.map((p) => p.provider),
    [
      'adobe-aio',
      'aem',
      'aws',
      'claude',
      'cloudflare',
      'elevenlabs',
      'fastly',
      'firefly',
      'garmin',
      'gcloud',
      'github',
      'gmail',
      'google',
      'huggingface',
      'kagi',
      'linkedin',
      'microsoft-graph',
      'npm',
      'openai',
      'printful',
      'rescuetime',
      'slack',
      'xai',
    ]
  );
  assert.equal(json.find((p) => p.provider === 'github').cli, true);
});

test('aem is a built-in oauth provider that needs org and site for create', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'aem']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /from = "body"/);
  assert.match(shown.stdout, /param = "authToken"/);
  assert.match(shown.stdout, /api\.aem\.live/);
  assert.match(shown.stdout, /cors_origins/);
  const missing = sb.jerk(['aem', 'create']);
  assert.equal(missing.status, 2);
  assert.match(missing.stderr, /-p org=/);
});

test('gmail is a built-in oauth provider for Google refresh tokens', () => {
  const sb = sandbox();
  const shown = sb.jerk(['config', 'show', 'gmail']);
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /GWS_ACCESS_TOKEN/);
  assert.match(shown.stdout, /GWS_REFRESH_TOKEN/);
  assert.match(shown.stdout, /access_type=offline/);
  assert.match(shown.stdout, /oauth2\.googleapis\.com\/token/);
  assert.match(shown.stdout, /oauth2\.googleapis\.com\/revoke/);
  assert.match(shown.stdout, /mail\.google\.com/);
  const missing = sb.jerk(['gmail', 'get']);
  assert.equal(missing.status, 3);
  assert.match(missing.stderr, /jerk gmail create/);
});

test('a user file replaces a built-in provider as a unit', () => {
  const sb = sandbox();
  sb.writeUser(
    '[providers.github]\nenv = ["MY_GH"]\n[providers.github.oauth]\nmode = "device"\nflow = "pkce"\nauthorize_url = "https://example.com/device"\ntoken_url = "https://example.com/token"\nclient_id = "c"\nredirect_uri = "http://127.0.0.1:1/cb"\n[providers.github.oauth.capture]\nat = "request"\nfrom = "query"\nparam = "code"\n'
  );
  const json = JSON.parse(sb.jerk(['list', '--json']).stdout);
  const github = json.find((p) => p.provider === 'github');
  assert.equal(github.source, 'user');
  assert.deepEqual(github.env, ['MY_GH']);
  assert.equal(github.requires, null);
  const check = sb.jerk(['config', 'check']);
  assert.equal(check.status, 0);
  assert.equal(check.stdout, 'ok: 23 providers (22 built-in, 1 user)\n');
});

test('config check reports every problem and exits 2', () => {
  const sb = sandbox();
  sb.writeUser('[providers.Bad]\nenv = ["1A"]\ndomains = ["*.x.com"]\n');
  const res = sb.jerk(['config', 'check']);
  assert.equal(res.status, 2);
  assert.match(res.stderr, /Bad: the id must be lower-case/);
  assert.match(res.stderr, /env name "1A"/);
  assert.match(res.stderr, /domain "\*\.x\.com" must be an exact host name/);
  assert.equal(sb.jerk(['list']).status, 2);
});

test('--config adds an explicit layer, and a missing one is an error', () => {
  const sb = sandbox();
  const missing = sb.jerk(['--config', `${sb.dir}/nope.toml`, 'config', 'check']);
  assert.equal(missing.status, 2);
  assert.match(missing.stderr, /cannot read/);
  const path = sb.jerk(['--config', `${sb.dir}/nope.toml`, 'config', 'path', '--json']);
  assert.equal(path.status, 0);
  assert.equal(JSON.parse(path.stdout).explicit.exists, false);
});

test('config path puts state and the system layer where the OS expects them', () => {
  const sb = sandbox();
  const res = sb.jerk(['config', 'path', '--json'], { SLICC_JERK_SYSTEM_DIR: '' });
  assert.equal(res.status, 0, res.stderr);
  const paths = JSON.parse(res.stdout);
  assert.equal(paths.state.path, sb.stateDir);
  const system =
    {
      darwin: '/Library/Application Support/slicc-jerk/providers.d',
      win32: 'C:\\ProgramData\\slicc-jerk\\providers.d',
    }[process.platform] ?? '/etc/slicc-jerk/providers.d';
  assert.equal(paths.system.path, system);
  assert.equal(paths.user.path, sb.userFile);
});

test('SLICC_JERK_STATE_DIR moves the state directory on every OS', () => {
  const sb = sandbox();
  const dir = `${sb.dir}/elsewhere`;
  const res = sb.jerk(['config', 'path', '--json'], { SLICC_JERK_STATE_DIR: dir });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(JSON.parse(res.stdout).state.path, dir);
});

test('config show prints the effective provider as TOML', () => {
  const sb = sandbox();
  const res = sb.jerk(['config', 'show', 'github']);
  assert.equal(res.status, 0);
  assert.match(res.stdout, /^\[providers\.github\]/m);
  assert.match(res.stdout, /run = \["gh", "auth", "token", "--hostname", "\{host\}"\]/);
  assert.equal(res.stderr, 'jerk: github comes from built-in\n');
});

test('intercept exits 2 when not on a terminal', () => {
  const sb = sandbox();
  const res = sb.jerk(['intercept']);
  assert.equal(res.status, 2);
  assert.match(res.stderr, /terminal/);
});
