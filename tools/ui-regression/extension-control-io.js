// Only the network client is controlled; session/selection, query, and hook are real.
export const extensionIo = { calls: [], pending: true, creates: [] };
export async function connect(host, port) {
  return {
    close() {}, whenClosed() {},
    async request(command, args) {
      extensionIo.calls.push({ host, command, args });
      if (command === 'getVirtualDisplay') return { supported: true, removalPending: extensionIo.pending, suggested: { width: 1280, height: 800, scale: 2, source: 'fallback' } };
      if (command === 'createVirtualDisplay') return new Promise((resolve, reject) => extensionIo.creates.push({ host, resolve, reject }));
      throw new Error(`Unexpected command ${command}`);
    },
  };
}
