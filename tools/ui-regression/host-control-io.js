export const transport = { attempts: [], clients: [], catalogs: [] };
export function connect(host, port) {
  return new Promise((resolve, reject) => {
    const client = { host, port, closed: false, closeCount: 0, close() { this.closed = true; this.closeCount++; }, request(method) {
      if (method !== 'getCatalog') throw new Error(`Unexpected Host fixture request: ${method}`);
      return new Promise((resolve, reject) => transport.catalogs.push({ host, resolve, reject }));
    } };
    transport.clients.push(client);
    transport.attempts.push({ host, resolve: () => resolve(client), reject });
  });
}
