#!/usr/bin/env node
import { appendFileSync } from 'node:fs';

const builds = [
  {
    name: 'darwin',
    runner: 'macos-latest',
    builds:
      'aarch64-apple-darwin:jerk:jerk-darwin-arm64 x86_64-apple-darwin:jerk:jerk-darwin-amd64',
  },
  {
    name: 'linux-arm64',
    runner: 'ubuntu-24.04-arm',
    builds: 'aarch64-unknown-linux-musl:jerk:jerk-linux-arm64',
  },
  {
    name: 'linux-amd64',
    runner: 'ubuntu-latest',
    builds: 'x86_64-unknown-linux-musl:jerk:jerk-linux-amd64',
  },
  {
    name: 'windows-arm64',
    runner: 'windows-11-arm',
    builds: 'aarch64-pc-windows-msvc:jerk.exe:jerk-windows-arm64.exe',
  },
  {
    name: 'windows-amd64',
    runner: 'windows-latest',
    builds: 'x86_64-pc-windows-msvc:jerk.exe:jerk-windows-amd64.exe',
  },
  { name: 'wasm', runner: 'ubuntu-latest', builds: 'wasm32-wasip1:jerk.wasm:jerk.wasm' },
];

const pullRequest = process.argv.includes('--pull-request');
const selected = pullRequest ? builds.filter((b) => !b.runner.startsWith('macos')) : builds;
const line = `matrix=${JSON.stringify({ include: selected })}\n`;
if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, line);
process.stdout.write(line);
