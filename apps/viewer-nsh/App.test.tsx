import { expect, open, test } from "nsh/test";

test("the page takes its title and text from the system language", async () => {
  const app = await open("./App.tsx", { locale: "en-US" });
  await app.waitFor("id:display-0");
  expect(app.texts()).toContain("Select Display to View");
  expect(app.texts()).toContain("Connected Computer");
  expect(app.texts()).toContain("#0 Primary");
});

test("the settings sheet switches the language of everything shown", async () => {
  const app = await open("./App.tsx", { locale: "ko-KR" });
  await app.waitFor("text:열어 볼 화면 선택");
  await app.press("id:settings");
  await app.waitFor("text:스트리밍 및 뷰어 설정");
  await app.press("id:language-en");
  expect(app.shows("text:Streaming & Viewer Settings")).toBe(true);
  expect(app.shows("text:Select Display to View")).toBe(true);
  await app.press("id:settings-close");
  await app.waitFor("text:Streaming & Viewer Settings", { state: "gone" });
  expect(app.storage()["leftcar.language"]).toBe("en");
});

test("the language chosen before is the one shown", async () => {
  const app = await open("./App.tsx", { locale: "ko-KR", storage: { "leftcar.language": "en" } });
  await app.waitFor("text:Select Display to View");
  expect(app.shows("text:열어 볼 화면 선택")).toBe(false);
});
