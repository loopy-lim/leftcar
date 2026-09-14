import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
if (process.platform !== 'darwin') throw new Error('Swift media interoperability requires macOS');
const directory = mkdtempSync(join(tmpdir(), 'leftcar-media-interop-'));

function run(command, args, capture = false) {
  const result = spawnSync(command, args, {
    cwd: root, encoding: 'utf8', stdio: capture ? 'pipe' : 'inherit',
  });
  if (capture && result.stderr) process.stderr.write(result.stderr);
  if (result.error || result.status !== 0) {
    if (capture && result.stdout) process.stdout.write(result.stdout);
    throw new Error(`${command} failed (${result.status}): ${result.error?.message ?? ''}`);
  }
  return result.stdout;
}

try {
  const messages = run('cargo', ['build', '-p', 'secure-channel', '--example', 'media_interop',
    '--locked', '--message-format=json'], true);
  const peer = messages.split('\n').filter(Boolean).map(line => JSON.parse(line))
    .find(message => message.reason === 'compiler-artifact'
      && message.target.name === 'media_interop' && message.executable)?.executable;
  if (!peer) throw new Error('Cargo did not return the media interoperability executable');
  const swiftTest = join(directory, 'media-interop-test');
  run('zsh', ['tools/build-macos-capture-shim.zsh', 'media-interop-test', swiftTest]);
  run(swiftTest, [peer, join(root, 'crates/secure-channel/tests/fixtures/media-wire-v1.txt')]);
} finally {
  rmSync(directory, { recursive: true, force: true });
}
