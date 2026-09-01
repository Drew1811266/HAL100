import { describe, expect, it } from "vitest";
import { isRuntimeProfileFailure } from "./desktop-api";

describe("runtime profile failure IPC contract", () => {
  it("recognizes the bounded structured failure returned by Rust", () => {
    expect(
      isRuntimeProfileFailure({
        code: "engineUnreachable",
        stage: "inspection",
        retryable: true,
        recoveryAction: "checkService",
      }),
    ).toBe(true);
  });

  it("rejects string and partial legacy errors", () => {
    expect(isRuntimeProfileFailure("外部推理引擎当前不可达")).toBe(false);
    expect(
      isRuntimeProfileFailure({
        code: "engineUnreachable",
        stage: "inspection",
      }),
    ).toBe(false);
    expect(
      isRuntimeProfileFailure({
        code: "inventedFailure",
        stage: "inspection",
        retryable: true,
        recoveryAction: "checkService",
      }),
    ).toBe(false);
  });
});
