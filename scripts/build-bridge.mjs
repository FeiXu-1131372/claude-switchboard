#!/usr/bin/env node
// Builds the statusline bridge and puts it where Tauri's `externalBin`
// expects it: src-tauri/binaries/switchboard-statusline-<target triple>.
//
// Run by `tauri build` through src-tauri/tauri.bundle.conf.json's
// beforeBuildCommand, which sets TAURI_ENV_TARGET_TRIPLE. By hand:
//   node scripts/build-bridge.mjs [target-triple]
//
// The bridge is built with its own cargo profile (`bridge`, panic = "unwind")
// because the release profile's `panic = "abort"` would defeat the
// catch_unwind that keeps a bug in it from ever touching a status line.

import { execFileSync } from 'node:child_process';
import { copyFileSync, chmodSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const tauriDir = join(root, 'src-tauri');
const host = /^host: (.+)$/m.exec(execFileSync('rustc', ['-vV']).toString())?.[1];
if (!host) throw new Error('could not read the host triple from `rustc -vV`');

const wanted = process.argv[2] || process.env.TAURI_ENV_TARGET_TRIPLE || host;
const exe = wanted.includes('windows') ? '.exe' : '';

/** Build for one concrete triple; returns the path of the binary. */
function build(triple) {
  const args = ['build', '-p', 'statusline-bridge', '--profile', 'bridge'];
  // Without --target, cargo writes to target/<profile>, not target/<triple>/<profile>.
  if (triple !== host) args.push('--target', triple);
  execFileSync('cargo', args, { cwd: tauriDir, stdio: 'inherit' });
  const dir = triple === host ? 'bridge' : join(triple, 'bridge');
  return join(tauriDir, 'target', dir, `switchboard-statusline${exe}`);
}

const outDir = join(tauriDir, 'binaries');
mkdirSync(outDir, { recursive: true });
const dest = join(outDir, `switchboard-statusline-${wanted}${exe}`);

if (wanted === 'universal-apple-darwin') {
  // Tauri bundles a universal app; the sidecar has to be universal too, so a
  // copy carried to an Intel Mac without Rosetta still runs.
  const arm = build('aarch64-apple-darwin');
  const intel = build('x86_64-apple-darwin');
  execFileSync('lipo', ['-create', '-output', dest, arm, intel], { stdio: 'inherit' });
} else {
  copyFileSync(build(wanted), dest);
}
if (!exe) chmodSync(dest, 0o755);
console.log(`bridge ready: ${dest}`);
