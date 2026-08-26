if (!globalThis.__leftcarUsbRuntime) {
  globalThis.__leftcarUsbRuntime = {
    getUsbNative: () => undefined,
    subscribeUsbNative: () => ({ remove: () => undefined }),
  };
}
