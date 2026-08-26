import { readFileSync } from "node:fs";
import { createFauxCore, fauxAssistantMessage } from "@earendil-works/pi-ai";
import { describe, expect, it } from "vitest";
import {
  AGENT_TASK_CLARIFICATION_KEYS,
  AGENT_TASK_KIND_KEYS,
  AGENT_TASK_REJECTION_KEYS,
  parseAgentIntentProposal,
  proposePiIntent,
  validateAgentIntentRequest,
} from "../src/agent-intent.js";
import {
  AGENT_MODEL_ALIAS,
  LOCAL_AGENT_MAX_OUTPUT_TOKENS,
  LOCAL_AGENT_STANDARD_CONTEXT_WINDOW_TOKENS,
} from "../src/agent-run.js";

const validRequest = {
  prompt: "把那个代码工具重新接到 HAL100 网关。",
  gatewayBaseUrl: "http://127.0.0.1:10100/v1",
  apiKey: "hal100_agent_test_key_1234567890",
  modelId: AGENT_MODEL_ALIAS,
  providerProtocol: "localOpenAi",
  contextWindowTokens: LOCAL_AGENT_STANDARD_CONTEXT_WINDOW_TOKENS,
  maxOutputTokens: LOCAL_AGENT_MAX_OUTPUT_TOKENS,
} as const;

describe("Pi structured Agent intent proposal", () => {
  it("matches every enum in the shared intent schema", () => {
    const schema = JSON.parse(
      readFileSync(
        new URL("../../../contracts/agent-intent/v1-schema.json", import.meta.url),
        "utf8",
      ),
    ) as {
      $defs: {
        taskKind: { enum: string[] };
        clarificationKind: { enum: string[] };
        rejectionReason: { enum: string[] };
      };
    };

    expect(AGENT_TASK_KIND_KEYS).toEqual(schema.$defs.taskKind.enum);
    expect(AGENT_TASK_CLARIFICATION_KEYS).toEqual(schema.$defs.clarificationKind.enum);
    expect(AGENT_TASK_REJECTION_KEYS).toEqual(schema.$defs.rejectionReason.enum);
  });

  it("accepts only one bounded canonical v1 proposal", () => {
    expect(
      parseAgentIntentProposal(
        JSON.stringify({
          schemaVersion: 1,
          disposition: "task",
          taskKind: "configure_external_agent",
          targetId: "opencode",
        }),
      ),
    ).toEqual({
      schemaVersion: 1,
      disposition: "task",
      taskKind: "configure_external_agent",
      targetId: "opencode",
    });

    for (const invalid of [
      '```json\n{"schemaVersion":1,"disposition":"unresolved"}\n```',
      JSON.stringify({ schemaVersion: 2, disposition: "unresolved" }),
      JSON.stringify({
        schemaVersion: 1,
        disposition: "task",
        taskKind: "run_shell",
      }),
      JSON.stringify({
        schemaVersion: 1,
        disposition: "reject",
        rejectionReason: "outside_capability_boundary",
        rationale: "arbitrary model text",
      }),
      "x".repeat(2 * 1024 + 1),
    ]) {
      expect(parseAgentIntentProposal(invalid)).toBeUndefined();
    }
  });

  it("uses Pi without tools and emits only the sanitized proposal", async () => {
    const faux = createFauxCore({
      api: "openai-completions",
      provider: "hal100-intent-test",
      models: [{ id: AGENT_MODEL_ALIAS }],
    });
    faux.setResponses([
      (context, _options, _state, requestModel) => {
        expect(context.tools ?? []).toEqual([]);
        expect(requestModel.maxTokens).toBe(128);
        expect(requestModel.samplingParams?.temperature).toBe(0);
        return fauxAssistantMessage(
          JSON.stringify({
            schemaVersion: 1,
            disposition: "task",
            taskKind: "configure_external_agent",
            targetId: "opencode",
          }),
          { stopReason: "stop" },
        );
      },
    ]);

    const result = await proposePiIntent(validRequest, {
      streamFn: faux.streamSimple,
      model: faux.getModel() as never,
    });

    expect(result).toEqual({
      status: "proposed",
      proposal: {
        schemaVersion: 1,
        disposition: "task",
        taskKind: "configure_external_agent",
        targetId: "opencode",
      },
    });
    expect(faux.state.callCount).toBe(1);
  });

  it("reduces invalid output and provider failures to fixed codes", async () => {
    const faux = createFauxCore({
      api: "openai-completions",
      provider: "hal100-intent-test",
      models: [{ id: AGENT_MODEL_ALIAS }],
    });
    faux.setResponses([fauxAssistantMessage("我认为应该配置。", { stopReason: "stop" })]);
    await expect(
      proposePiIntent(validRequest, {
        streamFn: faux.streamSimple,
        model: faux.getModel() as never,
      }),
    ).resolves.toEqual({ status: "invalid", errorCode: "invalid_intent_output" });

    await expect(
      proposePiIntent(validRequest, {
        streamFn: () => {
          throw new Error("sensitive provider failure");
        },
        model: faux.getModel() as never,
        failureCode: () => "gateway_unreachable",
      }),
    ).resolves.toEqual({ status: "failed", errorCode: "gateway_unreachable" });

    expect(validateAgentIntentRequest(validRequest).prompt).toBe(validRequest.prompt);
    expect(() => validateAgentIntentRequest({ ...validRequest, execute: true } as never)).toThrow(
      /shape/,
    );
  });
});
