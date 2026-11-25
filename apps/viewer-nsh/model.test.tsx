import { expect, test } from "nsh/test";
import { copy } from "./copy";
import { catalogFixture } from "./catalog/fixture";
import { createCatalogModel, InvalidCatalogAnswer, type CatalogSnapshot, type CatalogSource } from "./catalog/model";
import { parseCatalog } from "./catalog/source";

const STREAM = { session: 7, sourceIndex: 0, sourceName: "Studio Display", width: 2560, height: 1440, fps: 60, transport: "udp" as const };
const OTHER_STREAM = { ...STREAM, session: 8, sourceIndex: 1, sourceName: "MacBook Pro" };
const CATALOG = catalogFixture({ displays: catalogFixture().displays.slice(0, 2), streams: [] });
const DISPLAY = CATALOG.displays[0];
const withStreams = (...streams: CatalogSnapshot["streams"]): CatalogSnapshot => ({ ...CATALOG, streams });

interface Answer {
  promise: Promise<CatalogSnapshot>;
  resolve(catalog: CatalogSnapshot): void;
  reject(cause: unknown): void;
}

function answer(): Answer {
  let resolve!: Answer["resolve"];
  let reject!: Answer["reject"];
  const promise = new Promise<CatalogSnapshot>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

const letAnswersSettle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

function computer() {
  const asked: string[] = [];
  const answers = { load: [] as Answer[], open: [] as Answer[], stop: [] as Answer[] };
  const next = (name: string, queue: Answer[]) => {
    asked.push(name);
    const given = answer();
    queue.push(given);
    return given.promise;
  };
  const source: CatalogSource = {
    hostAddress: "192.168.0.10:7777",
    load: () => next("load", answers.load),
    open: () => next("open", answers.open),
    stop: (session) => next(`stop ${session}`, answers.stop),
  };
  return { model: createCatalogModel(source), asked, answers };
}

test("refresh: a catalog asked for before an open does not undo it", async () => {
  const { model, answers } = computer();
  answers.load[0].resolve(CATALOG);
  await letAnswersSettle();
  const refreshing = model.refresh();
  const opening = model.open(DISPLAY);
  answers.open[0].resolve(withStreams(STREAM));
  await opening;
  expect(model.snapshot()?.streams).toEqual([STREAM]);
  answers.load[1].resolve(CATALOG);
  await refreshing;
  expect(model.snapshot()?.streams).toEqual([STREAM]);
  expect(model.loading()).toBe(false);
});

test("refresh: one asked for while a display opens is not asked, and the next one is", async () => {
  const { model, asked, answers } = computer();
  answers.load[0].resolve(CATALOG);
  await letAnswersSettle();
  const opening = model.open(DISPLAY);
  await model.refresh();
  expect(asked).toEqual(["load", "open"]);
  expect(model.loading()).toBe(false);
  answers.open[0].resolve(withStreams(STREAM));
  await opening;
  const refreshing = model.refresh();
  expect(asked).toEqual(["load", "open", "load"]);
  expect(model.loading()).toBe(true);
  answers.load[1].resolve(withStreams(STREAM));
  await refreshing;
  expect(model.loading()).toBe(false);
});

test("refresh: one under way is not asked again", async () => {
  const { model, asked, answers } = computer();
  await model.refresh();
  expect(asked).toEqual(["load"]);
  answers.load[0].resolve(CATALOG);
  await letAnswersSettle();
  const second = model.refresh();
  expect(asked).toEqual(["load", "load"]);
  answers.load[1].resolve(CATALOG);
  await second;
  expect(model.loading()).toBe(false);
});

test("refresh: one under way when a display opens is superseded, and one asked for meanwhile is not asked", async () => {
  const { model, asked, answers } = computer();
  answers.load[0].resolve(CATALOG);
  await letAnswersSettle();
  const superseded = model.refresh();
  const opening = model.open(DISPLAY);
  await model.refresh();
  expect(model.loading()).toBe(false);
  expect(asked).toEqual(["load", "load", "open"]);
  answers.open[0].resolve(withStreams(STREAM));
  await opening;
  expect(asked).toEqual(["load", "load", "open"]);
  answers.load[1].resolve(CATALOG);
  await superseded;
  expect(model.snapshot()?.streams).toEqual([STREAM]);
  expect(model.loading()).toBe(false);
});

test("changes: one at a time, for each answers with the whole catalog", async () => {
  const { model, asked, answers } = computer();
  answers.load[0].resolve(withStreams(STREAM));
  await letAnswersSettle();
  const opening = model.open(CATALOG.displays[1]);
  await model.stop(STREAM.session);
  expect(asked).toEqual(["load", "open"]);
  expect(model.stoppingSession()).toBeNull();
  answers.open[0].resolve(withStreams(STREAM, OTHER_STREAM));
  await opening;
  const stopping = model.stop(STREAM.session);
  await model.open(CATALOG.displays[0]);
  expect(asked).toEqual(["load", "open", "stop 7"]);
  answers.stop[0].resolve(withStreams(OTHER_STREAM));
  await stopping;
  expect(model.snapshot()?.streams).toEqual([OTHER_STREAM]);
  const openingAgain = model.open(CATALOG.displays[0]);
  expect(asked).toEqual(["load", "open", "stop 7", "open"]);
  answers.open[1].resolve(CATALOG);
  await openingAgain;
});

test("open: a failure is forgotten by the next open", async () => {
  const { model, answers } = computer();
  answers.load[0].resolve(CATALOG);
  await letAnswersSettle();
  const failing = model.open(DISPLAY);
  answers.open[0].reject(new Error("화면을 열지 못했습니다"));
  await failing;
  expect(model.error()).toBe("화면을 열지 못했습니다");
  const working = model.open(DISPLAY);
  expect(model.error()).toBeNull();
  answers.open[1].resolve(withStreams(STREAM));
  await working;
  expect(model.error()).toBeNull();
  expect(model.snapshot()?.streams).toEqual([STREAM]);
});

test("open: a failure keeps its reason, and a refresh asked for meanwhile is not asked", async () => {
  const { model, asked, answers } = computer();
  answers.load[0].resolve(CATALOG);
  await letAnswersSettle();
  const failing = model.open(DISPLAY);
  await model.refresh();
  answers.open[0].reject(new Error("화면을 열지 못했습니다"));
  await failing;
  expect(asked).toEqual(["load", "open"]);
  expect(model.error()).toBe("화면을 열지 못했습니다");
  const retry = model.refresh();
  expect(model.error()).toBeNull();
  expect(asked).toEqual(["load", "open", "load"]);
  answers.load[1].resolve(CATALOG);
  await retry;
});

test("stop: a refresh asked for while a stream stops is not asked, an older error notwithstanding", async () => {
  const { model, asked, answers } = computer();
  answers.load[0].resolve(withStreams(STREAM));
  await letAnswersSettle();
  const failing = model.open(DISPLAY);
  answers.open[0].reject(new Error("화면을 열지 못했습니다"));
  await failing;
  const stopping = model.stop(STREAM.session);
  await model.refresh();
  answers.stop[0].resolve(CATALOG);
  await stopping;
  expect(asked).toEqual(["load", "open", "stop 7"]);
  expect(model.error()).toBe("화면을 열지 못했습니다");
  const retry = model.refresh();
  expect(asked).toEqual(["load", "open", "stop 7", "load"]);
  expect(model.error()).toBeNull();
  answers.load[1].resolve(CATALOG);
  await retry;
});

test("stop: a failure is said in the notice too, the computer's timeout in the app's words", async () => {
  const { model, answers } = computer();
  answers.load[0].resolve(withStreams(STREAM));
  await letAnswersSettle();
  const stopping = model.stop(STREAM.session);
  answers.stop[0].reject(Object.assign(new Error("host.call timed out"), { code: "ETIMEDOUT" }));
  await stopping;
  expect(model.stopFailed()).toEqual([STREAM.session]);
  expect(model.error()).toBe(copy.en.viewer.errHostTimeout);
  const retry = model.refresh();
  expect(model.error()).toBeNull();
  answers.load[1].resolve(withStreams(STREAM));
  await retry;
  expect(model.stopFailed()).toEqual([STREAM.session]);
});

test("stop: one stream's failure outlasts another's stop and its own retry", async () => {
  const { model, asked, answers } = computer();
  answers.load[0].resolve(withStreams(STREAM, OTHER_STREAM));
  await letAnswersSettle();
  const failing = model.stop(STREAM.session);
  answers.stop[0].reject(new Error("stream busy"));
  await failing;
  expect(model.stopFailed()).toEqual([STREAM.session]);
  const stopping = model.stop(OTHER_STREAM.session);
  await model.stop(STREAM.session);
  expect(asked).toEqual(["load", "stop 7", "stop 8"]);
  expect(model.stopFailed()).toEqual([STREAM.session]);
  answers.stop[1].resolve(withStreams(STREAM));
  await stopping;
  expect(model.stopFailed()).toEqual([STREAM.session]);
  const retry = model.stop(STREAM.session);
  expect(model.stopFailed()).toEqual([]);
  answers.stop[2].resolve(CATALOG);
  await retry;
  expect(model.stopFailed()).toEqual([]);
});

test("stop: a failure is forgotten once its stream is gone", async () => {
  const { model, answers } = computer();
  answers.load[0].resolve(withStreams(STREAM));
  await letAnswersSettle();
  const failing = model.stop(STREAM.session);
  answers.stop[0].reject(new Error("stream busy"));
  await failing;
  expect(model.stopFailed()).toEqual([STREAM.session]);
  const refreshing = model.refresh();
  answers.load[1].resolve(CATALOG);
  await refreshing;
  expect(model.stopFailed()).toEqual([]);
});

test("stop: each stream keeps its own failure", async () => {
  const { model, asked, answers } = computer();
  answers.load[0].resolve(withStreams(STREAM, OTHER_STREAM));
  await letAnswersSettle();
  const failing = model.stop(STREAM.session);
  answers.stop[0].reject(new Error("stream busy"));
  await failing;
  const alsoFailing = model.stop(OTHER_STREAM.session);
  await model.refresh();
  answers.stop[1].reject(new Error("stream busy"));
  await alsoFailing;
  expect(asked).toEqual(["load", "stop 7", "stop 8"]);
  expect(model.error()).toBe(copy.en.viewer.errGeneric);
  expect(model.stopFailed()).toEqual([STREAM.session, OTHER_STREAM.session]);
  const retry = model.stop(STREAM.session);
  expect(model.stopFailed()).toEqual([OTHER_STREAM.session]);
  answers.stop[2].resolve(withStreams(OTHER_STREAM));
  await retry;
  expect(model.stopFailed()).toEqual([OTHER_STREAM.session]);
});

test("errors: the app's words for what it knows of a failure, the computer's only when it is guidance", async () => {
  const { model, answers } = computer();
  const said: [unknown, string][] = [
    [new InvalidCatalogAnswer("catalog.streams"), copy.en.viewer.errGeneric],
    [Object.assign(new Error("host.call timed out"), { code: "ETIMEDOUT" }), copy.en.viewer.errHostTimeout],
    [new Error("컴퓨터에 연결하지 못했습니다"), "컴퓨터에 연결하지 못했습니다"],
    [new Error("ECONNREFUSED 127.0.0.1:7777 (internal stack)"), copy.en.viewer.errGeneric],
    ["stream busy", copy.en.viewer.errGeneric],
    [new Error("   "), copy.en.viewer.errGeneric],
    [{}, copy.en.viewer.errGeneric],
  ];
  for (const [cause, text] of said) {
    const asking = model.refresh();
    answers.load[answers.load.length - 1].reject(cause);
    await asking;
    expect(model.error()).toBe(text);
  }
});

test("parseCatalog: a catalog passes as it is", () => {
  expect(parseCatalog(withStreams(STREAM))).toEqual(withStreams(STREAM));
});

test("parseCatalog: what is not a catalog is refused by the first field that is off", () => {
  const verdict = (value: unknown) => {
    try {
      parseCatalog(value);
      return "taken";
    } catch (error) {
      return error instanceof InvalidCatalogAnswer ? error.path : String(error);
    }
  };
  expect(verdict(null)).toBe("catalog");
  expect(verdict({ fps: 60, displays: [] })).toBe("catalog.streams");
  expect(verdict({ ...CATALOG, fps: "60" })).toBe("catalog.fps");
  expect(verdict({ ...CATALOG, displays: "none" })).toBe("catalog.displays");
  expect(verdict({ ...CATALOG, displays: [{ index: 0, name: "Studio Display", width: 2560 }] })).toBe("catalog.displays[0].height");
  expect(verdict({ ...CATALOG, streams: [{ ...STREAM, transport: "quic" }] })).toBe("catalog.streams[0].transport");
  expect(verdict({ ...CATALOG, fps: -60.5 })).toBe("catalog.fps");
  expect(verdict({ ...CATALOG, fps: Number.NaN })).toBe("catalog.fps");
  expect(verdict({ ...CATALOG, displays: [{ ...DISPLAY, width: 0 }] })).toBe("catalog.displays[0].width");
  expect(verdict({ ...CATALOG, displays: [{ ...DISPLAY, index: -1.5 }] })).toBe("catalog.displays[0].index");
  expect(verdict({ ...CATALOG, displays: [DISPLAY, DISPLAY] })).toBe("catalog.displays[1]");
  expect(verdict(withStreams({ ...STREAM, session: -7.25 }))).toBe("catalog.streams[0].session");
  expect(verdict(withStreams(STREAM, STREAM))).toBe("catalog.streams[1]");
});
