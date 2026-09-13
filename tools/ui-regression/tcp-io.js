import { io } from "./viewer-io";
let session = 0;
const catalog = {
  platform: "macos",
  captureBackends: [{ id: "screenCaptureKit", label: "Screen", hint: "" }],
  displays: [{ sourceId: "macos:display:synthetic", index: 0, name: "Test display", width: 1920, height: 1080 }],
  encoderExperiments: [],
  reconfigureSource: true,
};
export default {
  createConnection(_options, connected) {
    const handlers = new Map();
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
      destroy() {},
      write(line, _encoding, complete) {
        const { command, args } = JSON.parse(line);
        io.controlCalls.push({command,args});
        const result =
          command === "getCatalog"
            ? (io.emptyCatalog ? {...catalog, displays: []} : catalog)
            : command === "getStatus"
              ? { sessions: [] }
              : command === "startStream" || command === "reconfigureStream"
                ? {
                    session: args.session ?? ++session,
                    width: args.width,
                    height: args.height,
                    fps: args.fps,
                  }
                : {};
        queueMicrotask(() => {
          complete?.();
          handlers.get("data")?.(JSON.stringify({ ok: true, result }) + "\n");
        });
        return true;
      },
    };
    queueMicrotask(connected);
    return socket;
  },
};
