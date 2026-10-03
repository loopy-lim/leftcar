export * from './viewer-io';
import { io } from './viewer-io';
export const storageIo = { holdRecentRead: false, reads: [] };
export async function deleteItemAsync(key) {
  if (io.failStorageDeletes?.has(key)) throw new Error('SecureStore delete failed');
  io.storage.delete(key);
}
export async function getItemAsync(key) {
  if (key === 'leftcar.recent_hosts' && storageIo.holdRecentRead) {
    return new Promise(resolve => storageIo.reads.push(() => resolve(io.storage.get(key) ?? null)));
  }
  return io.storage.get(key) ?? null;
}
