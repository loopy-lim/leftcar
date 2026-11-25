import { expect, open, test } from "nsh/test";
import { catalogFixture } from "./catalog/fixture";
import type { CatalogSnapshot } from "./catalog/model";

const KOREAN = { locale: "ko-KR" };
const HOST = { host: "192.168.0.10:7777" };
const STREAM = { session: 7, sourceIndex: 0, sourceName: "Studio Display", width: 2560, height: 1440, fps: 60, transport: "udp" as const };
const OTHER_STREAM = { ...STREAM, session: 8, sourceIndex: 1, sourceName: "MacBook Pro" };
const TWO_DISPLAYS = catalogFixture({ displays: catalogFixture().displays.slice(0, 2), streams: [] });
const GENERIC_ERROR = "문제가 발생했습니다. 잠시 후 다시 시도해 주세요.";
const STOP_FAILED = "이 화면을 종료하지 못했습니다. 다시 시도해 주세요.";

function answeredAfter(ms: number, answer: () => CatalogSnapshot) {
  return () => new Promise<CatalogSnapshot>((resolve) => setTimeout(() => resolve(answer()), ms));
}

function embedded(handlers: Record<string, unknown>) {
  return { ...KOREAN, host: { params: HOST, handlers } };
}

const whileItAnswers = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

test("standalone: a fixture stands in for the computer", async () => {
  const app = await open("./App.tsx", KOREAN);
  await app.waitFor("id:display-0");
  const texts = app.texts();
  expect(texts).toContain("Studio Display");
  expect(texts).toContain("2560 × 1440 · 60 FPS");
  expect(texts).toContain("#0 주 화면");
  expect(texts).toContain("#2 보조 화면");
  expect(texts).toContain("연결된 컴퓨터");
  expect(texts).toContain("192.168.0.10:7777");
  expect(texts).toContain("2560 × 1440 · 60 FPS · Wi-Fi");
  expect(app.shows("text:스트리밍 중")).toBe(true);
  expect(app.count("id:open-0")).toBe(0);
  expect(app.count("id:open-1")).toBe(1);
  expect(app.count("id:open-2")).toBe(1);
  expect(app.texts()).not.toContain("컴퓨터 변경");
  expect(app.count("id:change-host")).toBe(0);
});

test("loading: what the computer answered replaces the search", async () => {
  const app = await open("./App.tsx", embedded({ "catalog.load": answeredAfter(1500, () => TWO_DISPLAYS) }));
  await app.waitFor("id:display-0");
  expect(app.shows("text:사용할 수 있는 화면을 찾는 중입니다…")).toBe(false);
});

test("error: it shows what went wrong and tries again", async () => {
  let fail = true;
  const app = await open("./App.tsx", embedded({
    "catalog.load": () => {
      if (fail) throw new Error("컴퓨터에 연결하지 못했습니다");
      return TWO_DISPLAYS;
    },
  }));
  await app.waitFor("id:catalog-error");
  expect(app.texts()).toContain("컴퓨터에 연결하지 못했습니다");
  expect(app.shows("text:열 수 있는 화면이 없습니다.")).toBe(false);
  expect(app.count("id:displays-empty")).toBe(0);
  fail = false;
  await app.press("id:error-retry");
  await app.waitFor("id:display-0");
  expect(app.count("id:catalog-error")).toBe(0);
});

test("error: once the computer answers with nothing to open, the empty card comes back", async () => {
  let fail = true;
  const app = await open("./App.tsx", embedded({
    "catalog.load": () => {
      if (fail) throw new Error("컴퓨터에 연결하지 못했습니다");
      return catalogFixture({ displays: [], streams: [] });
    },
  }));
  await app.waitFor("id:catalog-error");
  expect(app.count("id:displays-empty")).toBe(0);
  fail = false;
  await app.press("id:error-retry");
  await app.waitFor("text:열 수 있는 화면이 없습니다.");
  expect(app.count("id:retry")).toBe(1);
  expect(app.count("id:catalog-error")).toBe(0);
});

test("refused: an answer that is not a catalog says so, and a later one is taken", async () => {
  let broken = true;
  const app = await open("./App.tsx", embedded({
    "catalog.load": () => (broken ? { fps: 60, displays: TWO_DISPLAYS.displays } : TWO_DISPLAYS),
  }));
  await app.waitFor("id:catalog-error");
  expect(app.texts()).toContain(GENERIC_ERROR);
  expect(app.count("id:display-0")).toBe(0);
  broken = false;
  await app.press("id:error-retry");
  await app.waitFor("id:display-0");
});

test("refused: an open answered with nothing keeps the list", async () => {
  const app = await open("./App.tsx", embedded({ "catalog.load": TWO_DISPLAYS, "catalog.open": () => null }));
  await app.waitFor("id:open-0");
  await app.press("id:open-0");
  await app.waitFor("id:catalog-error");
  expect(app.texts()).toContain(GENERIC_ERROR);
  expect(app.count("id:display-0")).toBe(1);
  expect(app.count("id:display-1")).toBe(1);
  expect(app.count("id:stream-7")).toBe(0);
});

test("empty: a computer with nothing to open offers to look again", async () => {
  const app = await open("./App.tsx", embedded({ "catalog.load": catalogFixture({ displays: [], streams: [] }) }));
  await app.waitFor("text:열 수 있는 화면이 없습니다.");
  expect(app.count("id:display-0")).toBe(0);
  expect(app.count("id:active-streams")).toBe(0);
  expect(app.count("id:retry")).toBe(1);
});

test("empty: while a stream stops, the empty card's retry is held too", async () => {
  const app = await open("./App.tsx", embedded({
    "catalog.load": { ...TWO_DISPLAYS, displays: [], streams: [STREAM] },
    "catalog.stop": answeredAfter(800, () => ({ ...TWO_DISPLAYS, displays: [], streams: [] })),
  }));
  await app.waitFor("id:retry");
  expect(app.enabled("id:retry")).toBe(true);
  const stopping = app.press("id:stop-7");
  await whileItAnswers(300);
  expect(app.enabled("id:retry")).toBe(false);
  await stopping;
  await app.waitFor("id:stream-7", { state: "gone" });
  expect(app.enabled("id:retry")).toBe(true);
});

test("standby: with no computer, the header says so", async () => {
  const app = await open("./App.tsx", { ...KOREAN, host: { handlers: { "catalog.load": TWO_DISPLAYS } } });
  await app.waitFor("id:display-0");
  expect(app.texts()).toContain("연결 안 됨");
  expect(app.texts()).toContain("—");
});

test("standby: a computer named with nothing is none", async () => {
  const app = await open("./App.tsx", { ...KOREAN, host: { params: { host: "" }, handlers: { "catalog.load": TWO_DISPLAYS } } });
  await app.waitFor("id:display-0");
  expect(app.texts()).toContain("연결 안 됨");
  expect(app.texts()).toContain("—");
});

test("refresh: the header's button asks the computer again", async () => {
  let fps = 60;
  const app = await open("./App.tsx", embedded({
    "catalog.load": () => ({ ...TWO_DISPLAYS, fps }),
  }));
  await app.waitFor("id:display-0");
  fps = 120;
  await app.press("id:refresh");
  await app.waitFor("text:2560 × 1440 · 120 FPS");
  expect(app.enabled("id:refresh")).toBe(true);
});

test("open: a failure is not shown once a later open works", async () => {
  let fail = true;
  const app = await open("./App.tsx", embedded({
    "catalog.load": TWO_DISPLAYS,
    "catalog.open": () => {
      if (fail) throw new Error("화면을 열지 못했습니다");
      return { ...TWO_DISPLAYS, streams: [STREAM] };
    },
  }));
  await app.waitFor("id:open-0");
  await app.press("id:open-0");
  await app.waitFor("id:catalog-error");
  expect(app.texts()).toContain("화면을 열지 못했습니다");
  fail = false;
  await app.press("id:open-0");
  await app.waitFor("id:stream-7");
  expect(app.count("id:catalog-error")).toBe(0);
});

test("open: a display opens, then streams, while the others wait", async () => {
  const app = await open("./App.tsx", embedded({
    "catalog.load": TWO_DISPLAYS,
    "catalog.open": answeredAfter(1500, () => ({ ...TWO_DISPLAYS, streams: [STREAM] })),
  }));
  await app.waitFor("id:open-0");
  const opening = app.press("id:open-0");
  await whileItAnswers(300);
  expect(app.shows("text:화면을 여는 중…")).toBe(true);
  expect(app.enabled("id:open-1")).toBe(false);
  await opening;
  await app.waitFor("text:스트리밍 중");
  expect(app.count("id:open-0")).toBe(0);
  expect(app.enabled("id:open-1")).toBe(true);
  expect(app.count("id:stream-7")).toBe(1);
});

test("stop: a stream that will not stop says so until it does", async () => {
  let fail = true;
  const app = await open("./App.tsx", embedded({
    "catalog.load": { ...TWO_DISPLAYS, streams: [STREAM] },
    "catalog.stop": () => {
      if (fail) throw new Error("stream busy");
      return TWO_DISPLAYS;
    },
  }));
  await app.waitFor("id:stop-7");
  await app.press("id:stop-7");
  await app.waitFor("text:이 화면을 종료하지 못했습니다. 다시 시도해 주세요.");
  expect(app.count("id:active-streams")).toBe(1);
  expect(app.count("id:catalog-error")).toBe(1);
  fail = false;
  await app.press("id:stop-retry-7");
  await app.waitFor("id:active-streams", { state: "gone" });
  expect(app.count("id:open-0")).toBe(1);
});

test("stop: one stream's failed stop outlasts another's, one stop at a time", async () => {
  const app = await open("./App.tsx", embedded({
    "catalog.load": { ...TWO_DISPLAYS, streams: [STREAM, OTHER_STREAM] },
    "catalog.stop": (args: { session: number }) => {
      if (args.session === 7) throw new Error("stream busy");
      return answeredAfter(800, () => ({ ...TWO_DISPLAYS, streams: [STREAM] }))();
    },
  }));
  await app.waitFor("id:stop-8");
  await app.press("id:stop-7");
  await app.waitFor(`text:${STOP_FAILED}`);
  const stopping = app.press("id:stop-8");
  await whileItAnswers(300);
  expect(app.enabled("id:stop-7")).toBe(false);
  expect(app.enabled("id:stop-retry-7")).toBe(false);
  await stopping;
  await app.waitFor("id:stream-8", { state: "gone" });
  expect(app.count("id:stream-7")).toBe(1);
  expect(app.shows(`text:${STOP_FAILED}`)).toBe(true);
  expect(app.enabled("id:stop-7")).toBe(true);
});

test("changes: nothing else changes while a display opens", async () => {
  const app = await open("./App.tsx", embedded({
    "catalog.load": { ...TWO_DISPLAYS, streams: [STREAM] },
    "catalog.open": answeredAfter(1500, () => ({ ...TWO_DISPLAYS, streams: [STREAM, OTHER_STREAM] })),
    "catalog.stop": () => { throw new Error("stream busy"); },
  }));
  await app.waitFor("id:stop-7");
  await app.press("id:stop-7");
  await app.waitFor(`text:${STOP_FAILED}`);
  const opening = app.press("id:open-1");
  await whileItAnswers(300);
  expect(app.enabled("id:open-1")).toBe(false);
  expect(app.enabled("id:stop-7")).toBe(false);
  expect(app.enabled("id:stop-retry-7")).toBe(false);
  expect(app.enabled("id:refresh")).toBe(false);
  await opening;
  await app.waitFor("id:stream-8");
  expect(app.enabled("id:stop-7")).toBe(true);
  expect(app.enabled("id:stop-retry-7")).toBe(true);
  expect(app.enabled("id:refresh")).toBe(true);
});

test("changes: nothing else changes while a stream stops", async () => {
  const app = await open("./App.tsx", embedded({
    "catalog.load": { ...TWO_DISPLAYS, streams: [STREAM] },
    "catalog.stop": answeredAfter(800, () => TWO_DISPLAYS),
  }));
  await app.waitFor("id:stop-7");
  const stopping = app.press("id:stop-7");
  await whileItAnswers(300);
  expect(app.enabled("id:stop-7")).toBe(false);
  expect(app.enabled("id:open-1")).toBe(false);
  expect(app.enabled("id:refresh")).toBe(false);
  await stopping;
  await app.waitFor("id:open-0");
  expect(app.enabled("id:open-1")).toBe(true);
  expect(app.enabled("id:refresh")).toBe(true);
});

test("stop: two streams that will not stop each say so, under their own name", async () => {
  const app = await open("./App.tsx", embedded({
    "catalog.load": { ...TWO_DISPLAYS, streams: [STREAM, OTHER_STREAM] },
    "catalog.stop": () => { throw new Error("stream busy"); },
  }));
  await app.waitFor("id:stop-8");
  await app.press("id:stop-7");
  await app.press("id:stop-8");
  await app.waitFor("id:stop-retry-8");
  expect(app.count("id:stop-retry-7")).toBe(1);
  expect(app.count(`text:${STOP_FAILED}`)).toBe(2);
});

test("stop: the notice's retry is held while a stream stops, then reads the catalog again", async () => {
  let loads = 0;
  const app = await open("./App.tsx", embedded({
    "catalog.load": () => { loads += 1; return { ...TWO_DISPLAYS, streams: loads === 1 ? [STREAM] : [] }; },
    "catalog.open": () => { throw new Error("display busy"); },
    "catalog.stop": answeredAfter(800, () => TWO_DISPLAYS),
  }));
  await app.waitFor("id:open-1");
  await app.press("id:open-1");
  await app.waitFor("id:catalog-error");
  const stopping = app.press("id:stop-7");
  await whileItAnswers(300);
  expect(app.enabled("id:error-retry")).toBe(false);
  await stopping;
  await app.waitFor("id:stream-7", { state: "gone" });
  expect(loads).toBe(1);
  await app.press("id:error-retry");
  await app.waitFor("id:catalog-error", { state: "gone" });
  expect(loads).toBe(2);
});

test("budgets: the computer gets 15 s to list and to open, 5 s to stop", async () => {
  const budgets: Record<string, number> = {};
  const catalog = (method: string) => ({ ...TWO_DISPLAYS, streams: method === "catalog.open" ? [STREAM] : [] });
  const app = await open("./App.tsx", { ...KOREAN, host: { params: HOST, handlers: { "catalog.load": TWO_DISPLAYS } }, modules: { nsh: {
    "host.call": (method: string, _params: unknown, options: { timeout: number }) => {
      budgets[method] = options.timeout;
      return Promise.resolve(catalog(method));
    },
  } } });
  await app.waitFor("id:open-0");
  await app.press("id:open-0");
  await app.waitFor("id:stop-7");
  await app.press("id:stop-7");
  await app.waitFor("id:open-0");
  await app.press("id:refresh");
  expect(budgets).toEqual({ "catalog.load": 15_000, "catalog.open": 15_000, "catalog.stop": 5_000 });
});

test("standby: a computer named with room around it is named without", async () => {
  const app = await open("./App.tsx", { ...KOREAN, host: { params: { host: "  192.168.0.10:7777  " }, handlers: { "catalog.load": TWO_DISPLAYS } } });
  await app.waitFor("id:display-0");
  expect(app.texts()).toContain("192.168.0.10:7777");
});
