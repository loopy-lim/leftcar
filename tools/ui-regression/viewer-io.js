// Only operating-system/storage boundaries are controlled. Session, transport
// framing, catalog model, stream controller, launch and reservation code are real.
export const io = {
  preparations: [],
  controlCalls: [],
  emptyCatalog: false,
  closes: [],
  opened: [],
  replies: [],
  presentationRequests: [],
  audioRequests: [],
  deferAudio: false,
  storage: new Map(),
  generation: new Map(),
};
export const NativeModules = {
  StreamLauncher: {
    setAudioStream(id, enabled) {
      return new Promise((resolve, reject) => io.audioRequests.push({ id, enabled, resolve, reject }));
    },
    setOpusAudio(id, enabled) {
      if (!io.deferAudio) return Promise.resolve();
      return new Promise((resolve, reject) => io.audioRequests.push({ id, enabled, resolve, reject }));
    },
    async getDecoderCapabilityHint() { return { maxInstances: 4 }; },
    setBalancedPresentation(id, balanced) {
      return new Promise((resolve, reject) => io.presentationRequests.push({ id, balanced, resolve, reject }));
    },
    prepareStream(port) {
      return new Promise((resolve) => io.preparations.push({ port, resolve }));
    },
    openStream(port) {
      io.generation.set(`src-${port}`, String(io.opened.length + 1));
      return new Promise((resolve, reject) => io.opened.push({ port, resolve, reject }));
    },
    async cancelPreparedStream() {},
    async getStreamGeneration(id) {
      return io.generation.get(id) ?? "";
    },
    closeStream(id, generation) {
      return new Promise((resolve, reject) =>
        io.closes.push({ id, generation, resolve, reject }),
      );
    },
  },
};
export const Alert = { alert() {} };
export const Platform = { OS: "android" };
export const router = { replace() {}, push() {} };
export const getItemAsync = async (key) => io.storage.get(key) ?? null;
export const setItemAsync = async (key, value) => { io.storage.set(key, value); };
export const deleteItemAsync = async () => {};
export const getStringAsync = async () => "";
export const setStringAsync = async () => true;
export const getImageAsync = async () => null;
export const setImageAsync = async () => {};
export default { deviceName: "Isolated Viewer" };

export const getRandomValues = (bytes) => crypto.getRandomValues(bytes);

export const AppState = { currentState: "active", addEventListener() { return { remove() {} }; } };
