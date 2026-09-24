export const transport = { attempts: [], clients: [], catalogs: [] };
export function connect(host, port) {
  return new Promise((resolve, reject) => {
    // ControlClient 계약(control.ts): close()는 오류·상대 종료와 마찬가지로
    // whenClosed 리스너를 발화한다. session.ts의 watchControlClose가 이를
    // 등록하므로 fixture 소켓도 계약을 갖춰야 connectHost가 진행된다.
    const closeListeners = new Set();
    const client = { host, port, closed: false, closeCount: 0, close() { this.closed = true; this.closeCount++; for (const listener of [...closeListeners]) listener(); }, transportFail() { this.closed = true; for (const listener of [...closeListeners]) listener(); }, whenClosed(listener) { closeListeners.add(listener); }, request(method) {
      if (method !== 'getCatalog') throw new Error(`Unexpected Host fixture request: ${method}`);
      return new Promise((resolve, reject) => transport.catalogs.push({ host, resolve, reject, socket: client }));
    } };
    transport.clients.push(client);
    const entry = { host, settled: false, resolve: null, reject: null };
    entry.resolve = () => { entry.settled = true; resolve(client); };
    entry.reject = (error) => { entry.settled = true; reject(error); };
    transport.attempts.push(entry);
  });
}
