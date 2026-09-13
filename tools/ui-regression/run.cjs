let chromium;
try {
  ({ chromium } = require(
    process.env.PLAYWRIGHT_CORE_PATH || "playwright-core",
  ));
} catch {
  throw new Error(
    "Install playwright-core or set PLAYWRIGHT_CORE_PATH to its installed package directory.",
  );
}
const assert = require("node:assert/strict");
(async () => {
  const browser = await chromium.launch({
    headless: true,
    ...(process.env.CHROMIUM_PATH
      ? { executablePath: process.env.CHROMIUM_PATH }
      : {}),
  });
  const failures = [];
  try {
    const page = await browser.newPage();
    const errors = [];
    page.on("pageerror", (error) => errors.push(String(error)));
    await page.goto(
      `file://${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/index.html`,
    );
    const check = async (name, test) => {
      if (process.env.UI_TEST_CASE && !name.includes(process.env.UI_TEST_CASE))
        return;
      try {
        await test();
        console.log(`PASS ${name}`);
      } catch (e) {
        failures.push(name);
        console.error(`FAIL ${name}: ${e.message}`);
      }
    };
    await check("independent initial privacy read", async () => {
      await page.locator("#lock").click();
      await page.evaluate(() =>
        window.settle("get_privacy_settings", [false, true]),
      );
      await page.waitForTimeout(30);
      assert.equal(await page.locator("#lock").textContent(), "true");
      assert.equal(await page.locator("#curtain").textContent(), "true");
    });
    await check(
      "pending independent setting and actionable save failure",
      async () => {
        await page.goto(`file://${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/index.html`);
        await page.locator("#lock").click();
        await page.evaluate(() => window.settle("get_privacy_settings", [false, true]));
        await page.waitForTimeout(30);
        const pending = await page.evaluate(() => ({
          lock: window.settings.lockPending,
          curtain: window.settings.curtainPending,
        }));
        assert.deepEqual(pending, { lock: true, curtain: false });
        await page.evaluate(() =>
          window.settle("set_lock_on_disconnect", "permission denied", true),
        );
        await page.waitForTimeout(30);
        const state = await page.evaluate(() => ({
          value: window.settings.lockOnDisconnect,
          pending: window.settings.lockPending,
          error: window.settings.lockError,
        }));
        assert.equal(state.value, false);
        assert.equal(state.pending, false);
        assert.match(state.error, /permission denied/);
      },
    );
    await check(
      "child Escape preserves ancestor and restores launcher focus",
      async () => {
        await page.locator("#open-parent").click();
        await page.locator("#open-child").click();
        assert.equal(
          await page
            .getByRole("dialog", { name: "Child", exact: true })
            .count(),
          1,
        );
        await page.keyboard.press("Escape");
        assert.equal(
          await page
            .getByRole("dialog", { name: "Parent", exact: true })
            .count(),
          1,
        );
        assert.equal(await page.locator("dialog[open]").count(), 1);
        assert.equal(
          await page.evaluate(() => document.activeElement.id),
          "open-child",
        );
        await page.keyboard.press("Escape");
        assert.equal(await page.locator("dialog[open]").count(), 0);
        assert.equal(
          await page.evaluate(() => document.activeElement.id),
          "open-parent",
        );
      },
    );
    await check(
      "actual dashboard callers expose pending, failure and dialog names",
      async () => {
        await page.goto(
          `file://${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/index.html?dashboard`,
        );
        const lock = page.getByRole("button", { name: /Lock on End/ });
        const curtain = page.getByRole("button", { name: /Privacy Curtain/ });
        await lock.click();
        assert.equal(await lock.isDisabled(), true);
        assert.equal(await lock.getAttribute("aria-busy"), "true");
        assert.equal(await curtain.isDisabled(), false);
        await page.evaluate(() =>
          window.settle("set_lock_on_disconnect", "permission denied", true),
        );
        await page.waitForTimeout(30);
        assert.equal(await lock.isDisabled(), false);
        assert.match(
          await page.getByRole("alert").textContent(),
          /permission denied/,
        );
        await page.getByRole("button", { name: "Retry", exact: true }).click();
        assert.equal(await lock.isDisabled(), true);
        await page.evaluate(() =>
          window.settle("set_lock_on_disconnect", null),
        );
        await page.waitForTimeout(30);
        assert.equal(await lock.getAttribute("aria-pressed"), "true");
        await page.evaluate(() =>
          window.settle("get_clipboard_share", "settings read failed", true),
        );
        await page.waitForTimeout(30);
        assert.match(
          await page.getByRole("alert").textContent(),
          /settings read failed/,
        );
        await page.getByRole("button", { name: "Retry", exact: true }).click();
        await page.waitForTimeout(30);
        await page.evaluate(() => window.settle("get_clipboard_share", true));
        await page.waitForTimeout(30);
        assert.equal(
          await page
            .getByRole("button", { name: /Clipboard Sharing/ })
            .getAttribute("aria-pressed"),
          "true",
        );
        await page.getByRole("button", { name: "Help", exact: true }).click();
        assert.equal(
          await page
            .getByRole("dialog", { name: "Troubleshooting Guide", exact: true })
            .count(),
          1,
        );
        await page.keyboard.press("Escape");
        await page
          .getByRole("button", { name: /Generate Pairing Code/ })
          .first()
          .click();
        assert.equal(
          await page
            .getByRole("dialog", { name: "Generate Pairing Code", exact: true })
            .count(),
          1,
        );
      },
    );
    await check(
      "actual catalog admission survives overlapping starts, unmount and late native cleanup",
      async () => {
        await page.goto(
          `file://${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/catalog.html`,
        );
        await page.waitForFunction(() => window.model?.displays.length === 1);
        await page.evaluate(() => {
          window.launches = Array.from({ length: 5 }, () =>
            window.model.openDisplay(window.model.displays[0]),
          );
        });
        await page.waitForFunction(
          () => window.viewerIo.preparations.length >= 4,
        );
        assert.equal(
          await page.evaluate(() => window.viewerIo.preparations.length),
          4,
        );
        await page.evaluate(() =>
          window.viewerIo.preparations.forEach((item) => item.resolve()),
        );
        await page.waitForFunction(() => window.viewerIo.opened.length === 4);
        await page.evaluate(() => window.mountCatalog(false));
        await page.waitForFunction(
          () => document.querySelector("output") === null,
        );
        await page.evaluate(() => window.mountCatalog(true));
        await page.waitForFunction(
          () => document.querySelector("output") !== null,
        );
        await page.evaluate(() =>
          window.model.openDisplay(window.model.displays[0]),
        );
        assert.equal(
          await page.evaluate(() => window.viewerIo.preparations.length),
          4,
        );
        await page.evaluate(() =>
          window.viewerIo.opened.forEach((item) =>
            item.resolve(`src-${item.port}`),
          ),
        );
        await page.waitForFunction(() => window.viewerIo.closes.length === 4);
        await page.evaluate(() =>
          window.model.openDisplay(window.model.displays[0]),
        );
        assert.equal(
          await page.evaluate(() => window.viewerIo.preparations.length),
          4,
        );
        await page.evaluate(() =>
          window.viewerIo.closes.forEach((item) => item.resolve()),
        );
        await page.evaluate(() => Promise.all(window.launches));
        await page.evaluate(() => {
          window.finalLaunch = window.model.openDisplay(
            window.model.displays[0],
          );
        });
        await page.waitForFunction(
          () => window.viewerIo.preparations.length === 5,
        );
        await page.evaluate(() => window.viewerIo.preparations[4].resolve());
        await page.waitForFunction(() => window.viewerIo.opened.length === 5);
        await page.evaluate(() =>
          window.viewerIo.opened[4].resolve("src-final"),
        );
        await page.evaluate(() => window.finalLaunch);
        assert.equal(await page.evaluate(() => window.model.streams.length), 1);
        await page.evaluate(() => {
          window.finalStop = window.model.stopStream(window.model.streams[0]);
        });
        await page.waitForFunction(() => window.viewerIo.closes.length === 5);
        assert.equal(await page.evaluate(() => window.model.streams.length), 1);
        await page.evaluate(() => window.viewerIo.closes[4].resolve());
        await page.evaluate(() => window.finalStop);
        await page.waitForFunction(() => window.model.streams.length === 0);
      },
    );
    await check("failed launch cleanup retries through Refresh while mounted", async () => {
      await page.goto(`file://${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/catalog.html`);
      await page.waitForFunction(() => window.model?.displays.length === 1);
      await page.evaluate(() => {
        window.failedLaunch = window.model.openDisplay(window.model.displays[0]);
      });
      await page.waitForFunction(() => window.viewerIo.preparations.length === 1);
      await page.evaluate(() => window.viewerIo.preparations[0].resolve());
      await page.waitForFunction(() => window.viewerIo.opened.length === 1);
      await page.evaluate(() => window.viewerIo.opened[0].reject(new Error("launch failed")));
      await page.waitForFunction(() => window.viewerIo.closes.length === 1);
      await page.evaluate(() => window.viewerIo.closes[0].reject(new Error("cleanup incomplete")));
      await page.evaluate(() => window.failedLaunch);
      assert.equal(await page.evaluate(() => window.model.streams.length), 0);
      await page.evaluate(() => {
        window.pendingLaunches = Array.from({length: 4}, () => window.model.openDisplay(window.model.displays[0]));
      });
      await page.waitForFunction(() => window.viewerIo.preparations.length >= 4);
      assert.equal(await page.evaluate(() => window.viewerIo.preparations.length), 4);
      await page.evaluate(() => window.model.handleRefresh());
      await page.waitForFunction(() => window.viewerIo.closes.length === 2, undefined, {timeout: 1500});
      assert.equal(await page.evaluate(() => window.viewerIo.preparations.length), 4);
      await page.evaluate(() => window.viewerIo.closes[1].resolve());
      await page.waitForTimeout(30);
      await page.evaluate(() => { window.recoveredLaunch = window.model.openDisplay(window.model.displays[0]); });
      await page.waitForFunction(() => window.viewerIo.preparations.length === 5);
    });
    // Fresh page resets actual hook/controller/session state and controlled I/O per case.
    const freshPresentationStreams = async (count = 1) => {
      await page.goto(`file://${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/catalog.html`);
      await page.waitForFunction(() => window.model?.displays.length === 1);
      for (let index = 0; index < count; index++) {
        await page.evaluate(() => { window.launch = window.model.openDisplay(window.model.displays[0]); });
        await page.waitForFunction((n) => window.viewerIo.preparations.length === n, index + 1);
        await page.evaluate((n) => window.viewerIo.preparations[n].resolve(), index);
        await page.waitForFunction((n) => window.viewerIo.opened.length === n, index + 1);
        await page.evaluate((n) => window.viewerIo.opened[n].resolve(`src-${window.viewerIo.opened[n].port}`), index);
        await page.evaluate(() => window.launch);
        await page.waitForFunction((n) => window.model.streams.length === n, index + 1);
      }
    };
    const presentationState = () => page.evaluate(() => ({
      requested: window.model.balancedPresentation,
      effective: window.model.streams.map((stream) => stream.balancedPresentation),
    }));
    const settlePresentation = async (index, failed = false) => {
      await page.evaluate(({index, failed}) => {
        const request = window.viewerIo.presentationRequests[index];
        if (failed) request.reject(new Error("presentation rejected"));
        else request.resolve();
      }, {index, failed});
      // React promise continuations and the preference persistence effect must commit.
      await page.waitForTimeout(30);
    };
    await check("actual catalog presentation success records effective mode only after completion", async () => {
      await freshPresentationStreams();
      await page.evaluate(() => window.model.handleToggleBalancedPresentation(true));
      assert.deepEqual(await presentationState(), {requested: true, effective: [false]});
      await settlePresentation(0);
      assert.deepEqual(await presentationState(), {requested: true, effective: [true]});
    });
    await check("actual catalog presentation reject retains previous effective mode and saved preference", async () => {
      await freshPresentationStreams();
      await page.evaluate(() => window.model.handleToggleBalancedPresentation(true));
      await settlePresentation(0);
      await page.evaluate(() => window.model.handleToggleBalancedPresentation(false));
      await settlePresentation(1, true);
      assert.deepEqual(await presentationState(), {requested: false, effective: [true]});
      assert.match(await page.evaluate(() => window.model.visibleError), /presentation rejected/);
      assert.equal(await page.evaluate(() => [...window.viewerIo.storage.values()].some((value) => value.includes('"balancedPresentation":false'))), true);
    });
    await check("actual catalog presentation mixed stream results commit independently", async () => {
      await freshPresentationStreams(2);
      await page.evaluate(() => window.model.handleToggleBalancedPresentation(true));
      await settlePresentation(0, true);
      await settlePresentation(1);
      assert.deepEqual(await presentationState(), {requested: true, effective: [false, true]});
    });
    await check("actual catalog presentation consecutive requests ignore stale success and error", async () => {
      await freshPresentationStreams();
      await page.evaluate(() => window.model.handleToggleBalancedPresentation(true));
      await page.evaluate(() => window.model.handleToggleBalancedPresentation(false));
      await settlePresentation(1);
      await settlePresentation(0);
      assert.deepEqual(await presentationState(), {requested: false, effective: [false]});
      await page.evaluate(() => window.model.handleToggleBalancedPresentation(true));
      await page.evaluate(() => window.model.handleToggleBalancedPresentation(false));
      await settlePresentation(3);
      await settlePresentation(2, true);
      assert.deepEqual(await presentationState(), {requested: false, effective: [false]});
      assert.equal(await page.evaluate(() => window.model.visibleError), null);
    });
    for (const earlierSuccessFirst of [true, false]) {
      await check(`actual catalog overlapping presentation success survives newer failure (${earlierSuccessFirst ? "success then failure" : "failure then success"})`, async () => {
        await freshPresentationStreams();
        await page.evaluate(() => {
          // Both calls are issued in one turn; neither native promise is settled.
          window.model.handleToggleBalancedPresentation(true);
          window.model.handleToggleBalancedPresentation(false);
        });
        assert.deepEqual(await page.evaluate(() => window.viewerIo.presentationRequests.map((request) => request.balanced)), [true, false]);
        await page.evaluate(async (successFirst) => {
          const [earlier, newer] = window.viewerIo.presentationRequests;
          if (successFirst) earlier.resolve();
          else newer.reject(new Error("newer presentation rejected"));
          await Promise.resolve();
          if (successFirst) newer.reject(new Error("newer presentation rejected"));
          else earlier.resolve();
        }, earlierSuccessFirst);
        await page.waitForFunction(() => window.model.visibleError?.includes("newer presentation rejected"));
        // Error observation proves settled callbacks committed; this assertion
        // cannot pass by waiting for an arbitrary inter-request delay.
        assert.deepEqual(await presentationState(), {requested: false, effective: [true]});
      });
    }
    for (const kind of ["localAudio", "opusAudio"]) {
      for (const successFirst of [true, false]) {
        await check(`actual catalog ${kind} older success survives newer failure (${successFirst})`, async () => {
          await freshPresentationStreams();
          await page.evaluate((kind) => {
            window.viewerIo.deferAudio = true;
            const toggle = kind === "localAudio" ? window.model.handleToggleAudio : window.model.handleToggleOpusAudio;
            toggle(kind !== "localAudio");
            toggle(kind === "localAudio");
          }, kind);
          await page.evaluate(async (successFirst) => {
            const [earlier, newer] = window.viewerIo.audioRequests;
            if (successFirst) earlier.resolve(); else newer.reject(new Error("newer audio rejected"));
            await Promise.resolve();
            if (successFirst) newer.reject(new Error("newer audio rejected")); else earlier.resolve();
          }, successFirst);
          await page.waitForFunction(() => window.model.visibleError?.includes("newer audio rejected"));
          const state = await page.evaluate((kind) => ({ requested: window.model[kind], effective: window.model.streams[0][kind] }), kind);
          assert.deepEqual(state, { requested: kind === "localAudio", effective: kind !== "localAudio" });
        });
      }
    }
    assert.deepEqual(errors, []);
    console.log(`Browser ${browser.version()}; failures ${failures.length}`);
    assert.deepEqual(failures, []);
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
