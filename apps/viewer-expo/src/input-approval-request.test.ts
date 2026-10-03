import { beforeEach, expect, it, vi } from "vitest";
import type { ActiveStream } from "./catalog-model-types";
import type { SessionRequestContext } from "./session";
import type { ReservedStream } from "./reserved-stream";

vi.mock("react-native-tcp-socket", () => ({
  default: { createConnection: vi.fn() },
}));
vi.mock("./session", () => ({
  bindRequestContext: vi.fn(),
  captureRequestContext: vi.fn(),
  reconnectHost: vi.fn(),
  isRequestContextCurrent: vi.fn(),
}));
import { captureRequestContext, isRequestContextCurrent } from "./session";
import { requestStreamInputApproval } from "./input-approval-request";
import { formatErrorMessage } from "./control";

const event = {
  port: 5001,
  instanceId: "src-5001",
  generation: "41",
  requestId: "3",
};
const target = { width: 1920, height: 1080, fps: 60 };
function stream(): ActiveStream {
  return {
    session: 8,
    port: 5001,
    startedAt: 10,
    sourceIndex: 0,
    sourceName: "Main",
    ...target,
    sourceTarget: target,
    activeTarget: target,
    fallbackTarget: null,
    qualityState: "native",
    captureBackend: "screenCaptureKit",
    contentMode: "interactive",
    encoderExperiment: "auto",
    mediaTransport: "udp",
    viewerIps: ["192.168.0.2"],
    mediaKey: "fixture",
    reservation: { generation: "41" } as ReservedStream,
  };
}
let selected: SessionRequestContext;
beforeEach(() => {
  selected = {
    client: {
      request: vi.fn().mockResolvedValue({ accepted: true }),
      close: vi.fn(),
      whenClosed: vi.fn(),
      hostKey: null,
    },
    target: { host: "host-a", port: 7777 },
    selectionGeneration: 1,
    identity: null,
    credential: null,
  };
  vi.mocked(captureRequestContext).mockImplementation(() => selected);
  vi.mocked(isRequestContextCurrent).mockImplementation(
    (context) => context === selected,
  );
});

it("a Host request failure reaches the initiating foreground window with its request id", async () => {
  const active = stream();
  vi.mocked(selected.client.request).mockRejectedValue(
    new Error("Host rejected request"),
  );
  const reportResult = vi.fn().mockResolvedValue(undefined);
  await requestStreamInputApproval({
    host: "host-a:7777",
    event,
    getSnapshot: () => [active],
    reportResult,
  });
  expect(reportResult).toHaveBeenCalledExactlyOnceWith(
    event,
    formatErrorMessage(new Error("Host rejected request")),
  );
});

it("successful delivery reports pending Host approval without enabling input itself", async () => {
  const active = stream();
  const reportResult = vi.fn().mockResolvedValue(undefined);
  await requestStreamInputApproval({
    host: "host-a:7777",
    event,
    getSnapshot: () => [active],
    reportResult,
  });
  expect(selected.client.request).toHaveBeenCalledExactlyOnceWith(
    "requestInputEnable",
    { session: 8 },
  );
  expect(reportResult).toHaveBeenCalledExactlyOnceWith(event, null);
});

it("an event from an older native generation cannot request access for a replaced window", async () => {
  const reportResult = vi.fn().mockResolvedValue(undefined);
  await requestStreamInputApproval({
    host: "host-a:7777",
    event: { ...event, generation: "40" },
    getSnapshot: () => [stream()],
    reportResult,
  });
  expect(selected.client.request).not.toHaveBeenCalled();
  expect(reportResult).not.toHaveBeenCalled();
});

it("late Host failures cannot change another window reusing the same port", async () => {
  const active = stream();
  let snapshot = [active];
  let reject!: (error: Error) => void;
  vi.mocked(selected.client.request).mockReturnValue(
    new Promise((_, fail) => {
      reject = fail;
    }),
  );
  const reportResult = vi.fn().mockResolvedValue(undefined);
  const pending = requestStreamInputApproval({
    host: "host-a:7777",
    event,
    getSnapshot: () => snapshot,
    reportResult,
  });
  snapshot = [{ ...stream(), session: 9, startedAt: 20 }];
  reject(new Error("late failure"));
  await pending;
  expect(reportResult).not.toHaveBeenCalled();
});

it("late success after a Host selection change does not publish into retained native chrome", async () => {
  const active = stream();
  let resolve!: () => void;
  vi.mocked(selected.client.request).mockReturnValue(
    new Promise((done) => {
      resolve = () => done({});
    }),
  );
  const reportResult = vi.fn().mockResolvedValue(undefined);
  const pending = requestStreamInputApproval({
    host: "host-a:7777",
    event,
    getSnapshot: () => [active],
    reportResult,
  });
  selected = {
    ...selected,
    target: { host: "host-b", port: 7777 },
    selectionGeneration: 2,
  };
  resolve();
  await pending;
  expect(reportResult).not.toHaveBeenCalled();
});
