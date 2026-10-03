import { io } from "./viewer-io";
let session = 0;
const catalog = {
  platform: "macos",
  captureBackends: [{ id: "screenCaptureKit", label: "Screen", hint: "" }],
  displays: globalThis.__viewerCatalogDisplays ?? [{ sourceId: "macos:display:synthetic", index: 0, name: "Test display", width: 1920, height: 1080 }],
  encoderExperiments: [],
  reconfigureSource: true,
};
export default {
  createConnection(_options, connected) {
    const handlers = new Map();
    const responses = [];
    let closed = false;
    const drain = () => {
      while (!closed && responses[0]?.ready) {
        const response = responses.shift();
        response.complete?.();
        handlers.get("data")?.(JSON.stringify(response.error
          ? { ok: false, error: response.error } : { ok: true, result: response.result }) + "\n");
      }
    };
    const socket = {
      on(name, handler) {
        handlers.set(name, handler);
        return socket;
      },
      once(name, handler) {
        handlers.set(name, handler);
        return socket;
      },
      setTimeout() {},
      setNoDelay() {},
      destroy() { closed = true; handlers.get("close")?.(); },
      write(line, _encoding, complete) {
        const { command, args } = JSON.parse(line);
        io.controlCalls.push({command,args});
        const result =
          command === "getCatalog"
            ? (io.emptyCatalog ? {...catalog, displays: []} : catalog)
            : command === "getStatus"
              ? (io.statusView ?? { sessions: [] })
              : command === "startStream" || command === "reconfigureStream"
                ? {
                    session: args.session ?? ++session,
                    width: args.width,
                    height: args.height,
                    fps: args.fps,
                  }
                : {};
        const response = { ready: false, complete, result, error: undefined };
        responses.push(response);
        // The real Host dispatches and replies in FIFO order. A deferred stop
        // must block later status responses rather than acknowledge that stop.
        const reply = (error) => { response.ready = true; response.error = error; drain(); };
        if (io.deferredCommands.has(command)) io.pendingCommands.push({ command, args, reply });
        else queueMicrotask(reply);
        return true;
      },
    };
    queueMicrotask(connected);
    return socket;
  },
};
