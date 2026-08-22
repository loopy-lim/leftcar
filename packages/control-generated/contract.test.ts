import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = "packages/control-generated";

describe("generated control contract (docs/04 §11)", () => {
  it("video_payload_type_is_absent_from_generated_typescript", () => {
    for (const side of ["host", "viewer"]) {
      const types = readFileSync(join(root, side, "types.ts"), "utf8");
      const commands = readFileSync(join(root, side, "commands.ts"), "utf8");
      const surface = types + commands;
      for (const banned of ["EncodedFrame", "NalUnit", "VideoPacket"]) {
        expect(surface, `${side} contract`).not.toContain(banned);
      }
    }
  });

  it("viewer_contract_has_no_high_rate_input_commands", () => {
    const types = readFileSync(join(root, "viewer", "types.ts"), "utf8");
    const commands = readFileSync(join(root, "viewer", "commands.ts"), "utf8");
    const surface = (types + commands).toLowerCase();
    for (const banned of ["sendkeyboard", "sendmouse", "injectinput", "clipboard"]) {
      expect(surface).not.toContain(banned);
    }
  });

  it("add_numbers_command_exists_in_host_types", () => {
    const types = readFileSync(join(root, "host", "types.ts"), "utf8");
    expect(types).toContain("AddNumbersInput");
    expect(types).toContain("AddNumbersOutput");
  });

  it("contract_hash_constant_is_present", () => {
    for (const side of ["host", "viewer"]) {
      const contract = readFileSync(join(root, side, "contract.ts"), "utf8");
      expect(contract).toContain("GENERATED_CONTRACT_HASH");
    }
  });
});
