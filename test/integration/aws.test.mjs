import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

const CREDS = {
  Version: 1,
  AccessKeyId: 'ASIAFAKEACCESSKEYID01',
  SecretAccessKey: 'wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY',
  SessionToken: 'FwoGZXIvYXdzEHsaDEXAMPLESESSIONTOKEN',
  Expiration: '2099-01-01T00:00:00Z',
};

const IDENTITY = {
  UserId: 'AIDAEXAMPLE',
  Account: '123456789012',
  Arn: 'arn:aws:iam::123456789012:user/jerk',
};

function fakeAws(sb) {
  sb.script(
    'aws',
    `const joined = args.join(' ');
if (joined.startsWith('configure export-credentials')) {
  process.stdout.write(${JSON.stringify(JSON.stringify(CREDS))});
} else if (joined.startsWith('sts get-caller-identity')) {
  process.stdout.write(${JSON.stringify(JSON.stringify(IDENTITY))});
} else if (joined.startsWith('sso login') || joined.startsWith('sso logout')) {
  process.stderr.write('fake ' + joined + '\\n');
} else process.exit(9);`
  );
}

test('aws get prints the access key id and maps lease credentials', () => {
  const sb = sandbox();
  fakeAws(sb);
  const masked = sb.jerk(['aws', 'get']);
  assert.equal(masked.status, 0, masked.stderr);
  assert.equal(masked.stdout, 'ASIA…ID01\n');
  const revealed = sb.jerk(['aws', 'get', '--reveal']);
  assert.equal(revealed.stdout, `${CREDS.AccessKeyId}\n`);
  const json = JSON.parse(sb.jerk(['aws', 'get', '--json']).stdout);
  assert.equal(json.provider, 'aws');
  assert.equal(json.source, 'cli');
  assert.equal(json.masked, 'ASIA…ID01');
  assert.equal(json.expires_at, '2099-01-01T00:00:00Z');
  assert.deepEqual(sb.calls()[0].args, ['configure', 'export-credentials']);
  const lease = JSON.parse(sb.jerk(['aws', 'lease'], { FNOX_LEASE_DURATION: '600' }).stdout);
  assert.deepEqual(lease.credentials, {
    AWS_ACCESS_KEY_ID: CREDS.AccessKeyId,
    AWS_SECRET_ACCESS_KEY: CREDS.SecretAccessKey,
    AWS_SESSION_TOKEN: CREDS.SessionToken,
  });
});

test('aws env wins over the CLI, including sibling AWS_* values for lease', () => {
  const sb = sandbox();
  fakeAws(sb);
  const fromEnv = sb.jerk(['aws', 'get', '--json'], {
    AWS_ACCESS_KEY_ID: 'AKIAGENVACCESSKEYID99',
    AWS_SECRET_ACCESS_KEY: 'envSecretKeyValueMustBeLong',
    AWS_SESSION_TOKEN: 'envSessionTokenValue',
  });
  assert.equal(fromEnv.status, 0, fromEnv.stderr);
  assert.equal(JSON.parse(fromEnv.stdout).source, 'env');
  assert.equal(sb.calls().length, 0);
  const lease = JSON.parse(
    sb.jerk(['aws', 'lease'], {
      FNOX_LEASE_DURATION: '600',
      AWS_ACCESS_KEY_ID: 'AKIAGENVACCESSKEYID99',
      AWS_SECRET_ACCESS_KEY: 'envSecretKeyValueMustBeLong',
      AWS_SESSION_TOKEN: 'envSessionTokenValue',
    }).stdout
  );
  assert.deepEqual(lease.credentials, {
    AWS_ACCESS_KEY_ID: 'AKIAGENVACCESSKEYID99',
    AWS_SECRET_ACCESS_KEY: 'envSecretKeyValueMustBeLong',
    AWS_SESSION_TOKEN: 'envSessionTokenValue',
  });
});

test('aws list parses sts get-caller-identity', () => {
  const sb = sandbox();
  fakeAws(sb);
  const list = sb.jerk(['aws', 'list', '--json']);
  assert.equal(list.status, 0, list.stderr);
  assert.deepEqual(JSON.parse(list.stdout).items[0], IDENTITY);
});

test('aws create and drop call sso login and logout, with optional profile', () => {
  const sb = sandbox();
  fakeAws(sb);
  const created = sb.jerk(['aws', 'create', '--force', '-p', 'profile=dev']);
  assert.equal(created.status, 0, created.stderr);
  assert.deepEqual(sb.calls().at(-1).args, ['sso', 'login', '--profile', 'dev']);
  const dropped = sb.jerk(['aws', 'drop', '--yes', '-p', 'profile=dev']);
  assert.equal(dropped.status, 0, dropped.stderr);
  assert.deepEqual(sb.calls().at(-1).args, ['sso', 'logout', '--profile', 'dev']);
});

test('aws get without the CLI still works from env', () => {
  const sb = sandbox();
  const res = sb.jerk(['aws', 'get', '--json'], {
    AWS_ACCESS_KEY_ID: 'AKIAGENVACCESSKEYID99',
  });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(JSON.parse(res.stdout).source, 'env');
});

test('aws get without the CLI and without env exits 4', () => {
  const sb = sandbox();
  const res = sb.jerk(['aws', 'get']);
  assert.equal(res.status, 4);
  assert.match(res.stderr, /aws is not on PATH/);
});
