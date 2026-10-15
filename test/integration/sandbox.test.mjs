import assert from 'node:assert/strict';
import { existsSync, readdirSync } from 'node:fs';
import { test } from 'node:test';
import { sandbox } from './helpers.mjs';

let dir;

test('a sandbox exists while its test runs', () => {
  const sb = sandbox();
  sb.writeUser('');
  sb.script('fake', '');
  dir = sb.dir;
  assert.ok(existsSync(dir));
  assert.ok(readdirSync(dir).length > 0);
});

test('a sandbox is gone once its test has ended', () => {
  assert.equal(existsSync(dir), false);
});
