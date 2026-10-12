#!/usr/bin/env node

const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const tty = require('node:tty');

const targets = new Set([
  'darwin-arm64',
  'darwin-x64',
  'linux-arm64',
  'linux-x64',
  'win32-arm64',
  'win32-x64',
]);

function fail(message) {
  process.stderr.write(`jerk: ${message}\n`);
  process.exit(1);
}

function binaryPath() {
  const target = `${process.platform}-${process.arch}`;
  if (!targets.has(target)) fail(`no jerk binary is published for ${target}`);
  const pkg = `@ai-ecoverse/slicc-jerk-${target}`;
  const file = process.platform === 'win32' ? 'jerk.exe' : 'jerk';
  let bin;
  try {
    bin = require.resolve(`${pkg}/bin/${file}`);
  } catch {
    fail(`${pkg} is not installed; reinstall without --omit=optional or --no-optional`);
  }
  if (process.platform !== 'win32') {
    try {
      fs.accessSync(bin, fs.constants.X_OK);
    } catch {
      try {
        fs.chmodSync(bin, 0o755);
      } catch {}
    }
  }
  return bin;
}

const forwarded = process.platform === 'win32' ? [] : ['SIGTERM', 'SIGHUP', 'SIGQUIT', 'SIGUSR2'];
const swallowed = process.platform === 'win32' ? ['SIGINT', 'SIGBREAK'] : [];
if (process.platform !== 'win32') {
  (tty.isatty(0) ? swallowed : forwarded).push('SIGINT');
}

const child = spawn(binaryPath(), process.argv.slice(2), {
  stdio: 'inherit',
  env: { ...process.env, JERK_LAUNCHER: 'npm' },
});

const handlers = new Map();
for (const signal of forwarded) {
  handlers.set(signal, () => {
    if (child.exitCode === null && child.signalCode === null) child.kill(signal);
  });
}
for (const signal of swallowed) handlers.set(signal, () => {});
for (const [signal, handler] of handlers) process.on(signal, handler);

child.on('error', (err) => fail(err.message));
child.on('exit', (code, signal) => {
  for (const [name, handler] of handlers) process.removeListener(name, handler);
  if (signal) {
    process.exitCode = 128 + (os.constants.signals[signal] || 0);
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code === null ? 1 : code);
});
