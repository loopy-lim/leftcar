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
    await check("privacy settings cannot change before the stored value is known", async () => {
      await page.goto(`file://${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/index.html`);
      await page.locator("#curtain-toggle").click();
      assert.equal(await page.evaluate(() => window.pendingCount("set_privacy_curtain")), 0);
      assert.equal(await page.evaluate(() => window.settings.curtainPending), true);
      await page.evaluate(() => window.settle("get_privacy_settings", true));
      await page.waitForFunction(() => window.settings.curtainPending === false);
      await page.locator("#curtain-toggle").click();
      // The curtain remains confirmed on until the disable command succeeds.
      assert.equal(await page.locator("#curtain").textContent(), "true");
      await page.evaluate(() => window.settle("set_privacy_curtain", null));
      await page.waitForFunction(() => window.settings.privacyCurtain === false);
      await page.goto(`file://${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/index.html`);
    });
    await check("initial privacy curtain read", async () => {
      await page.evaluate(() =>
        window.settle("get_privacy_settings", true),
      );
      await page.waitForTimeout(30);
      assert.equal(await page.locator("#curtain").textContent(), "true");
    });
    await check(
      "pending privacy curtain and actionable save failure",
      async () => {
        await page.goto(`file://${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/index.html`);
        await page.evaluate(() => window.settle("get_privacy_settings", false));
        await page.waitForFunction(() => window.settings.curtainPending === false);
        await page.locator("#curtain-toggle").click();
        await page.waitForTimeout(30);
        assert.equal(await page.evaluate(() => window.settings.curtainPending), true);
        await page.evaluate(() =>
          window.settle("set_privacy_curtain", "permission denied", true),
        );
        await page.waitForTimeout(30);
        const state = await page.evaluate(() => ({
          value: window.settings.privacyCurtain,
          pending: window.settings.curtainPending,
          error: window.settings.curtainError,
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
        // 설정 토글은 대시보드 본문이 아니라 Host Settings 모달 안에 산다.
        await page
          .getByRole("button", { name: "Host Settings", exact: true })
          .click();
        const curtain = page.getByRole("switch", {
          name: "Privacy Curtain", exact: true,
        });
        assert.equal(await curtain.isDisabled(), true);
        await page.evaluate(() => window.settle("get_privacy_settings", false));
        await page.waitForFunction(() => document.querySelector('button[role="switch"][aria-label="Privacy Curtain"]')?.disabled === false);
        await curtain.click();
        assert.equal(await curtain.isDisabled(), true);
        assert.equal(await curtain.getAttribute("aria-busy"), "true");
        await page.evaluate(() =>
          window.settle("set_privacy_curtain", "permission denied", true),
        );
        await page.waitForTimeout(30);
        assert.equal(await curtain.isDisabled(), true);
        assert.match(
          await page.getByRole("alert").textContent(),
          /permission denied/,
        );
        await page.getByRole("button", { name: "Retry", exact: true }).click();
        assert.equal(await curtain.isDisabled(), true);
        await page.evaluate(() =>
          window.settle("set_privacy_curtain", null),
        );
        await page.waitForTimeout(30);
        assert.equal(await curtain.getAttribute("aria-checked"), "true");
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
            .getByRole("switch", { name: "Clipboard Sharing", exact: true })
            .getAttribute("aria-checked"),
          "true",
        );
        // 모달 밖 대시보드 버튼(Help·페어링)은 설정 모달을 닫은 뒤 누른다.
        await page.keyboard.press("Escape");
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
    await check("actual catalog preserves preferences when storage reads fail", async () => {
      const catalogPage = await browser.newPage();
      try {
        await catalogPage.addInitScript(() => {
          window.__viewerStorageReadFailures = ["leftcar.viewerPreferences", "leftcar.clipboardShare", "leftcar.udpStability"];
          window.__viewerStoredValues = {
            "leftcar.viewerPreferences": JSON.stringify({ profileId: "balanced", localAudio: false, localCursor: false, showFps: true }),
            "leftcar.clipboardShare": "1",
            "leftcar.udpStability": JSON.stringify({ profile: "stable" }),
          };
        });
        await catalogPage.goto(`file://${process.env.UI_TEST_DIR || "/tmp/leftcar-task4-ui"}/catalog.html`);
        await catalogPage.waitForFunction(() => window.model?.displays.length === 1);
        await catalogPage.waitForTimeout(50);
        assert.deepEqual(await catalogPage.evaluate(() => window.viewerIo.storageWrites), []);
        assert.equal(await catalogPage.evaluate(() => window.model.viewerPreferenceControlsDisabled), true);
        assert.equal(await catalogPage.evaluate(() => window.model.clipboardPreferenceControlDisabled), true);
        assert.equal(await catalogPage.evaluate(() => window.model.udpPreferenceControlsDisabled), true);
        await catalogPage.evaluate(() => {
          window.model.handleToggleFps(false);
          window.model.handleToggleClipboardShare(false);
          window.model.handleSelectUdpStability({ profile: "auto" });
        });
        assert.deepEqual(await catalogPage.evaluate(() => window.viewerIo.storageWrites), []);
        await catalogPage.evaluate(() => {
          window.viewerIo.failStorageReads.clear();
          window.model.retryPersistence();
        });
        await catalogPage.waitForFunction(() => !window.model.viewerPreferenceControlsDisabled && !window.model.clipboardPreferenceControlDisabled && !window.model.udpPreferenceControlsDisabled);
        assert.equal(await catalogPage.evaluate(() => window.model.profileId), "balanced");
        assert.equal(await catalogPage.evaluate(() => window.model.showFps), true);
        assert.equal(await catalogPage.evaluate(() => window.model.clipboardShare), true);
        assert.equal(await catalogPage.evaluate(() => JSON.parse(window.viewerIo.storage.get("leftcar.udpStability")).profile), "stable");
        assert.deepEqual(await catalogPage.evaluate(() => window.viewerIo.storageWrites), []);
        await catalogPage.evaluate(() => {
          window.viewerIo.failStorageWrites.add("leftcar.viewerPreferences");
          window.model.handleToggleFps(false);
        });
        await catalogPage.waitForFunction(() => window.model.preferencePersistenceIssue === "viewer-save");
        assert.equal(await catalogPage.evaluate(() => JSON.parse(window.viewerIo.storage.get("leftcar.viewerPreferences")).showFps), true);
        await catalogPage.evaluate(() => {
          window.viewerIo.failStorageWrites.clear();
          window.model.retryPersistence();
        });
        await catalogPage.waitForFunction(() => window.model.preferencePersistenceIssue === null);
        assert.equal(await catalogPage.evaluate(() => JSON.parse(window.viewerIo.storage.get("leftcar.viewerPreferences")).showFps), false);
      } finally { await catalogPage.close(); }
    });
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
      assert.match(await page.evaluate(() => window.model.nativeSettingsFailures.find(failure => failure.key === "balanced")?.error), /presentation rejected/);
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
      assert.deepEqual(await page.evaluate(() => window.model.nativeSettingsFailures), []);
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
        await page.waitForFunction(() => window.model.nativeSettingsFailures.some(failure => failure.key === "balanced" && failure.error.includes("newer presentation rejected")));
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
          await page.waitForFunction(key => window.model.nativeSettingsFailures.some(failure => failure.key === key && failure.error.includes("newer audio rejected")), kind === "localAudio" ? "audio" : "opus");
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
