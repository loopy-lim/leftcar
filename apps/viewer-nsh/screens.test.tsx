import { expect, open, test, type App } from "nsh/test";

async function saveIfDrawn(app: App, name: string): Promise<void> {
  try {
    await app.screenshot(`screens/${name}.png`);
  } catch (error) {
    if (!(error instanceof Error) || !error.message.includes("needs a real platform")) throw error;
  }
}

const KOREAN = { locale: "ko-KR" };

test("screen: catalog, light", async () => {
  const app = await open("./App.tsx", KOREAN);
  await app.waitFor("id:display-0");
  await saveIfDrawn(app, "catalog-light");
});

test("screen: catalog, dark", async () => {
  const app = await open("./App.tsx", { ...KOREAN, colorScheme: "dark" });
  await app.waitFor("id:display-0");
  await saveIfDrawn(app, "catalog-dark");
});

test("screen: searching for displays", async () => {
  const app = await open("./App.tsx", { ...KOREAN, clock: {} });
  await app.waitFor("text:사용할 수 있는 화면을 찾는 중입니다…");
  await saveIfDrawn(app, "catalog-searching");
  await app.wait(1000);
  await app.waitFor("id:display-0");
  expect(app.shows("text:사용할 수 있는 화면을 찾는 중입니다…")).toBe(false);
});

test("screen: the settings sheet", async () => {
  const app = await open("./App.tsx", KOREAN);
  await app.waitFor("id:settings");
  await app.press("id:settings");
  await app.waitFor("text:스트리밍 및 뷰어 설정");
  await saveIfDrawn(app, "settings-sheet");
  expect(app.shows("text:한국어")).toBe(true);
});
