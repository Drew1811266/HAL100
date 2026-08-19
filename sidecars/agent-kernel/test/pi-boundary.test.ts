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
});
