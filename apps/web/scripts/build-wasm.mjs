import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const webRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const crateRoot = path.resolve(webRoot, '../../crates/crypto-wasm')
const cargoTarget = path.resolve(webRoot, '.wasm-target')
const outputFromCrate = path.relative(
  crateRoot,
  path.resolve(webRoot, 'public/wasm'),
)
const outputDirectory = path.resolve(webRoot, 'public/wasm')
const wasmPackVersion = spawnSync('wasm-pack', ['--version'], {
  encoding: 'utf8',
})
if (
  wasmPackVersion.status !== 0 ||
  wasmPackVersion.stdout.trim() !== 'wasm-pack 0.15.0'
) {
  console.error('wasm-pack 0.15.0 is required for reproducible builds')
  process.exit(1)
}
fs.rmSync(outputDirectory, { recursive: true, force: true })
const rustupRustc = spawnSync('rustup', ['which', 'rustc'], {
  encoding: 'utf8',
})
const rustupBin =
  rustupRustc.status === 0 ? path.dirname(rustupRustc.stdout.trim()) : undefined

const result = spawnSync(
  'wasm-pack',
  [
    'build',
    '--target',
    'web',
    '--release',
    '--no-pack',
    '--out-name',
    'sprout_crypto',
    '--out-dir',
    outputFromCrate,
    '--',
    '--locked',
  ],
  {
    cwd: crateRoot,
    stdio: 'inherit',
    env: {
      ...process.env,
      CARGO_TARGET_DIR: cargoTarget,
      PATH: rustupBin
        ? `${rustupBin}${path.delimiter}${process.env.PATH ?? ''}`
        : process.env.PATH,
      SOURCE_DATE_EPOCH: process.env.SOURCE_DATE_EPOCH ?? '0',
      CARGO_INCREMENTAL: '0',
      LC_ALL: 'C',
      TZ: 'UTC',
    },
  },
)

if (result.error) {
  throw result.error
}
if (result.status !== 0) {
  process.exit(result.status ?? 1)
}

fs.rmSync(path.join(outputDirectory, '.gitignore'), { force: true })
