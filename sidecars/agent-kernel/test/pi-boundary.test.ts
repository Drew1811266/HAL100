import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { piIntegrationPolicy, probePiKernel } from "../src/pi-boundary.js";

describe("HAL100 Pi boundary", () => {
  it("loads the real Pi Agent runtime with no tools or discovery capabilities", () => {
    expect(probePiKernel()).toEqual({
      piEnabled: true,
      registeredToolCount: 0,
      ...piIntegrationPolicy,
    });
  });

  it("depends on Pi Core libraries without embedding the official Pi Coding Agent", () => {
    const packageJson = JSON.parse(
      readFileSync(new URL("../package.json", import.meta.url), "utf8"),
    ) as { dependencies: Record<string, string> };

    expect(packageJson.dependencies["@earendil-works/pi-agent-core"]).toBe("0.84.2");
    expect(packageJson.dependencies["@earendil-works/pi-ai"]).toBe("0.84.2");
    expect(packageJson.dependencies["@earendil-works/pi-coding-agent"]).toBeUndefined();
  });
});
