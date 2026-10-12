export const TARGETS = [
  {
    id: 'darwin-arm64',
    asset: 'darwin-arm64',
    rust: 'aarch64-apple-darwin',
    os: 'darwin',
    cpu: 'arm64',
    label: 'macOS on Apple silicon',
  },
  {
    id: 'darwin-x64',
    asset: 'darwin-amd64',
    rust: 'x86_64-apple-darwin',
    os: 'darwin',
    cpu: 'x64',
    label: 'macOS on Intel',
  },
  {
    id: 'linux-arm64',
    asset: 'linux-arm64',
    rust: 'aarch64-unknown-linux-musl',
    os: 'linux',
    cpu: 'arm64',
    label: 'Linux on arm64',
  },
  {
    id: 'linux-x64',
    asset: 'linux-amd64',
    rust: 'x86_64-unknown-linux-musl',
    os: 'linux',
    cpu: 'x64',
    label: 'Linux on x86-64',
  },
  {
    id: 'win32-arm64',
    asset: 'windows-arm64',
    rust: 'aarch64-pc-windows-msvc',
    os: 'win32',
    cpu: 'arm64',
    label: 'Windows on arm64',
  },
  {
    id: 'win32-x64',
    asset: 'windows-amd64',
    rust: 'x86_64-pc-windows-msvc',
    os: 'win32',
    cpu: 'x64',
    label: 'Windows on x86-64',
  },
];

export const SCOPE = '@ai-ecoverse';
export const NAME = `${SCOPE}/slicc-jerk`;

export function assetName(target) {
  return `jerk-${target.asset}${target.os === 'win32' ? '.exe' : ''}`;
}
