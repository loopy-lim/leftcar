import { describe, expect, it } from "vitest";
import { resolveCameraState } from "./cameraPermission";

describe("resolveCameraState", () => {
  it("returns loading when the permission has not been fetched yet", () => {
    expect(resolveCameraState(null)).toBe("loading");
    expect(resolveCameraState(undefined)).toBe("loading");
  });

  it("returns request when permission can still be asked", () => {
    expect(resolveCameraState({ granted: false, canAskAgain: true })).toBe("request");
  });

  it("returns blocked when permission was permanently denied", () => {
    expect(resolveCameraState({ granted: false, canAskAgain: false })).toBe("blocked");
  });

  it("returns live when permission is granted", () => {
    expect(resolveCameraState({ granted: true, canAskAgain: false })).toBe("live");
  });
});
