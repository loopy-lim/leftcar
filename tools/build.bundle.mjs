import { cp, rename, rm, lstat } from 'node:fs/promises';
import { dirname, basename, join, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';
import { execFileSync } from 'node:child_process';

const exists = async path => { try { return await lstat(path); } catch (e) { if (e.code === 'ENOENT') return null; throw e; } };
// Copy and verify beside the destination before moving the existing bundle.
// A failed replacement restores the old verified bundle. A successful update
// retains it at backup for explicit rollback; nothing deletes that backup.
export async function replaceBundle(source, destination, verify) {
  if (resolve(source) === resolve(destination)) throw new Error('Source and destination must differ');
  const old = await exists(destination);
  if (old?.isSymbolicLink()) throw new Error('Destination must not be a symlink');
  if (old) await verify(destination);
  const id = randomUUID();
  const staged = join(dirname(destination), `.${basename(destination)}.staged-${id}`);
  const backup = `${destination}.previous-${id}`;
  let moved = false;
  try {
    await cp(source, staged, { recursive: true, errorOnExist: true, force: false });
    await verify(staged);
    if (old) { await rename(destination, backup); moved = true; }
    await rename(staged, destination);
    await verify(destination);
    return { installed: destination, backup: moved ? backup : null };
  } catch (error) {
    if (moved) {
      try { await rm(destination, { recursive: true, force: true }); await rename(backup, destination); }
      catch (rollback) { throw new AggregateError([error, rollback], `Replacement and rollback failed; previous bundle retained at ${backup}`); }
    }
    throw error;
  } finally { await rm(staged, { recursive: true, force: true }); }
}
export function verifySignedBundle(path) {
  execFileSync('/usr/bin/codesign', ['--verify', '--deep', '--strict', path], { stdio: 'pipe' });
}
if (import.meta.main) {
  const [source, destination] = process.argv.slice(2);
  if (!source || !destination) throw new Error('Usage: bun tools/build.bundle.mjs <verified-source.app-or-backup> <destination.app>');
  verifySignedBundle(source);
  // The updater has checked designated identity and fresh shim bytes. This CLI
  // also verifies seals for direct explicit rollback; it never launches an app.
  if (await exists(destination)) {
    const readRequirement = path => {
      const p = Bun.spawnSync(['/usr/bin/codesign', '-d', '-r-', path]);
      if (p.exitCode) throw new Error('Cannot read signing requirement');
      const line = p.stderr.toString().match(/^designated => .+$/m)?.[0];
      if (!line) throw new Error('Missing designated signing requirement');
      return line;
    };
    if (readRequirement(source) !== readRequirement(destination)) throw new Error('Signing requirements differ; refusing replacement');
  }
  console.log(JSON.stringify(await replaceBundle(source, destination, verifySignedBundle), null, 2));
}
