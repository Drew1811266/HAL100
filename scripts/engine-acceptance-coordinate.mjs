const ENGINE_WIRE_KEYS = Object.freeze({
  ollama: "ollama",
  "mlx-lm": "mlxLm",
  "mlc-llm": "mlcLlm",
  openvino: "openVino",
  vllm: "vllm",
  sglang: "sglang",
  lmdeploy: "lmDeploy",
  "tensorrt-llm": "tensorRtLlm",
});

const TARGET_COORDINATES = Object.freeze({
  "macos-arm64": Object.freeze({ platform: "macOs", architecture: "aarch64" }),
  "linux-x64": Object.freeze({ platform: "linux", architecture: "x86_64" }),
  "linux-arm64": Object.freeze({ platform: "linux", architecture: "aarch64" }),
  "windows-x64": Object.freeze({ platform: "windows", architecture: "x86_64" }),
});

const ACCELERATOR_WIRE_KEYS = Object.freeze({
  cpu: "cpu",
  metal: "metal",
  cuda: "cuda",
  rocm: "rocm",
  vulkan: "vulkan",
  intel_gpu: "intelGpu",
  intel_npu: "intelNpu",
});

const ACCEPTANCE_ENVIRONMENT_SPECS = Object.freeze({
  ollama: Object.freeze({
    apiRoot: "HAL100_OLLAMA_API_ROOT",
    modelId: "HAL100_OLLAMA_MODEL_ID",
    engineVersion: "HAL100_OLLAMA_EXPECTED_VERSION",
    accelerator: "HAL100_OLLAMA_ACCELERATOR",
    allowedAccelerators: Object.freeze(["cpu", "metal"]),
    optional: Object.freeze([]),
  }),
  "mlx-lm": Object.freeze({
    apiRoot: "HAL100_MLX_LM_API_ROOT",
    modelId: "HAL100_MLX_LM_MODEL_ID",
    engineVersion: "HAL100_MLX_LM_EXPECTED_VERSION",
    accelerator: undefined,
    allowedAccelerators: Object.freeze([]),
    optional: Object.freeze([]),
  }),
  "mlc-llm": Object.freeze({
    apiRoot: "HAL100_MLC_LLM_API_ROOT",
    modelId: "HAL100_MLC_LLM_MODEL_ID",
    engineVersion: "HAL100_MLC_LLM_ENGINE_VERSION",
    accelerator: "HAL100_MLC_LLM_ACCELERATOR",
    allowedAccelerators: Object.freeze(["metal", "vulkan", "cuda", "rocm"]),
    optional: Object.freeze([]),
  }),
  openvino: Object.freeze({
    apiRoot: "HAL100_OPENVINO_API_ROOT",
    modelId: "HAL100_OPENVINO_MODEL_ID",
    engineVersion: "HAL100_OPENVINO_EXPECTED_VERSION",
    accelerator: "HAL100_OPENVINO_ACCELERATOR",
    allowedAccelerators: Object.freeze(["cpu", "intel_gpu", "intel_npu"]),
    optional: Object.freeze([]),
  }),
  vllm: Object.freeze({
    apiRoot: "HAL100_VLLM_API_ROOT",
    modelId: "HAL100_VLLM_MODEL_ID",
    engineVersion: "HAL100_VLLM_EXPECTED_VERSION",
    accelerator: undefined,
    allowedAccelerators: Object.freeze([]),
    optional: Object.freeze(["HAL100_VLLM_API_KEY"]),
  }),
  sglang: Object.freeze({
    apiRoot: "HAL100_SGLANG_API_ROOT",
    modelId: "HAL100_SGLANG_MODEL_ID",
    engineVersion: "HAL100_SGLANG_EXPECTED_VERSION",
    accelerator: undefined,
    allowedAccelerators: Object.freeze([]),
    optional: Object.freeze([]),
  }),
  lmdeploy: Object.freeze({
    apiRoot: "HAL100_LMDEPLOY_API_ROOT",
    modelId: "HAL100_LMDEPLOY_MODEL_ID",
    engineVersion: undefined,
    accelerator: "HAL100_LMDEPLOY_ACCELERATOR",
    allowedAccelerators: Object.freeze(["cuda"]),
    optional: Object.freeze([]),
  }),
  "tensorrt-llm": Object.freeze({
    apiRoot: "HAL100_TENSORRT_LLM_API_ROOT",
    modelId: "HAL100_TENSORRT_LLM_MODEL_ID",
    engineVersion: "HAL100_TENSORRT_LLM_EXPECTED_VERSION",
    accelerator: undefined,
    allowedAccelerators: Object.freeze([]),
    optional: Object.freeze([]),
  }),
});

export const acceptanceEngineKeys = Object.freeze(Object.keys(ENGINE_WIRE_KEYS));
export const acceptanceTargetPlatforms = Object.freeze(Object.keys(TARGET_COORDINATES));
export const acceptanceAcceleratorKeys = Object.freeze(Object.keys(ACCELERATOR_WIRE_KEYS));

function hasAsciiControlCharacters(value) {
  return Array.from(value).some((character) => {
    const codePoint = character.codePointAt(0);
    return codePoint <= 0x1f || codePoint === 0x7f;
  });
}

function requiredBoundedEnvironmentValue(environment, name, maxBytes) {
  const value = environment[name];
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    Buffer.byteLength(value, "utf8") > maxBytes ||
    hasAsciiControlCharacters(value)
  ) {
    throw new Error(`${name} must be present and bounded`);
  }
  return value;
}

function optionalBoundedEnvironmentValue(environment, name, maxBytes) {
  const value = environment[name];
  if (value === undefined || value === "") {
    return;
  }
  requiredBoundedEnvironmentValue(environment, name, maxBytes);
}

function validateLoopbackApiRoot(value, variableName) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error(`${variableName} must be a valid loopback API root`);
  }
  if (
    parsed.protocol !== "http:" ||
    parsed.hostname !== "127.0.0.1" ||
    parsed.port.length === 0 ||
    parsed.username.length > 0 ||
    parsed.password.length > 0 ||
    parsed.search.length > 0 ||
    parsed.hash.length > 0 ||
    !parsed.pathname.startsWith("/") ||
    !parsed.pathname.endsWith("/")
  ) {
    throw new Error(
      `${variableName} must be an explicit 127.0.0.1 HTTP origin with a port and trailing slash`,
    );
  }
}

export function acceptanceEnvironmentSpec(engine) {
  const spec = ACCEPTANCE_ENVIRONMENT_SPECS[engine];
  if (!spec) {
    throw new Error("unsupported acceptance engine environment");
  }
  return spec;
}

// Validate protected/local environment wiring without issuing a network request or echoing any
// value. Rust remains authoritative for the native host, adapter and live service qualification.
export function validateAcceptanceEnvironment(engine, environment) {
  const spec = acceptanceEnvironmentSpec(engine);
  const apiRoot = requiredBoundedEnvironmentValue(environment, spec.apiRoot, 2048);
  validateLoopbackApiRoot(apiRoot, spec.apiRoot);
  requiredBoundedEnvironmentValue(environment, spec.modelId, 4096);
  if (spec.engineVersion) {
    requiredBoundedEnvironmentValue(environment, spec.engineVersion, 512);
  }
  if (spec.accelerator) {
    const accelerator = requiredBoundedEnvironmentValue(environment, spec.accelerator, 32);
    if (!spec.allowedAccelerators.includes(accelerator)) {
      throw new Error(`${spec.accelerator} does not name an allowed accelerator for ${engine}`);
    }
  }
  for (const optionalVariable of spec.optional) {
    optionalBoundedEnvironmentValue(environment, optionalVariable, 4096);
  }
  const requiredVariables = [spec.apiRoot, spec.modelId];
  if (spec.engineVersion) requiredVariables.push(spec.engineVersion);
  if (spec.accelerator) requiredVariables.push(spec.accelerator);
  return {
    schemaVersion: 1,
    engine,
    loopbackConfigurationValidated: true,
    requiredVariables,
    optionalVariables: [...spec.optional],
  };
}

function reverseLookup(entries, value, label) {
  const matches = Object.entries(entries).filter(([, candidate]) => {
    if (typeof candidate === "string") {
      return candidate === value;
    }
    return candidate.platform === value.platform && candidate.architecture === value.architecture;
  });
  if (matches.length !== 1) {
    throw new Error(`unsupported ${label}`);
  }
  return matches[0][0];
}

export function workflowEngineFromWire(engine) {
  return reverseLookup(ENGINE_WIRE_KEYS, engine, "engine wire key");
}

export function workflowTargetFromSupportCell(platform, architecture) {
  return reverseLookup(TARGET_COORDINATES, { platform, architecture }, "platform coordinate");
}

export function workflowAcceleratorFromWire(accelerator) {
  return reverseLookup(ACCELERATOR_WIRE_KEYS, accelerator, "accelerator wire key");
}

export function resolveAcceptanceCoordinate(matrix, selection) {
  if (matrix?.schemaVersion !== 1 || !Array.isArray(matrix.engines)) {
    throw new Error("unsupported inference-engine support matrix");
  }
  const engine = ENGINE_WIRE_KEYS[selection.engine];
  const target = TARGET_COORDINATES[selection.targetPlatform];
  const accelerator = ACCELERATOR_WIRE_KEYS[selection.accelerator];
  if (!engine || !target || !accelerator) {
    throw new Error("unknown live-acceptance selection");
  }

  const matches = matrix.engines.flatMap((entry) => {
    if (entry.engine !== engine) {
      return [];
    }
    return entry.supportUnits
      .filter(
        (unit) =>
          unit.platform === target.platform &&
          unit.architecture === target.architecture &&
          unit.accelerator === accelerator &&
          unit.deployment === "local",
      )
      .map((unit) => ({
        adapterVariant: entry.adapterVariant,
        contractRevision: entry.contractRevision,
        status: unit.status,
      }));
  });
  if (matches.length !== 1) {
    throw new Error("live-acceptance selection must resolve to exactly one declared support cell");
  }

  return {
    schemaVersion: 1,
    engine: selection.engine,
    adapterVariant: matches[0].adapterVariant,
    contractRevision: matches[0].contractRevision,
    targetPlatform: selection.targetPlatform,
    accelerator: selection.accelerator,
    manifestStatus: matches[0].status,
    deployment: "local",
  };
}

export function buildAcceptanceTargets(matrix, { includeFormal = false } = {}) {
  if (matrix?.schemaVersion !== 1 || !Array.isArray(matrix.engines)) {
    throw new Error("unsupported inference-engine support matrix");
  }
  const targets = [];
  for (const entry of matrix.engines) {
    if (entry.engine === "llamaCpp") {
      continue;
    }
    const engine = workflowEngineFromWire(entry.engine);
    for (const unit of entry.supportUnits) {
      const formal = unit.status === "managed" || unit.status === "verifiedExternal";
      if (formal && !includeFormal) {
        continue;
      }
      const targetPlatform = workflowTargetFromSupportCell(unit.platform, unit.architecture);
      const accelerator = workflowAcceleratorFromWire(unit.accelerator);
      const coordinate = resolveAcceptanceCoordinate(matrix, {
        engine,
        targetPlatform,
        accelerator,
      });
      if (
        coordinate.adapterVariant !== entry.adapterVariant ||
        coordinate.contractRevision !== entry.contractRevision
      ) {
        throw new Error("support cell resolved to a different adapter identity");
      }
      const requiredSecrets = ["HAL100_ACCEPTANCE_API_ROOT", "HAL100_ACCEPTANCE_MODEL_ID"];
      if (engine !== "lmdeploy") {
        requiredSecrets.push("HAL100_ACCEPTANCE_ENGINE_VERSION");
      }
      targets.push({
        engine,
        adapterVariant: entry.adapterVariant,
        contractRevision: entry.contractRevision,
        targetPlatform,
        accelerator,
        manifestStatus: unit.status,
        environmentName: `hal100-acceptance-${targetPlatform}-${engine}-${accelerator}`,
        requiredSecrets,
        optionalSecrets: engine === "vllm" ? ["HAL100_VLLM_API_KEY"] : [],
      });
    }
  }
  targets.sort(
    (left, right) =>
      left.targetPlatform.localeCompare(right.targetPlatform) ||
      left.engine.localeCompare(right.engine) ||
      left.accelerator.localeCompare(right.accelerator),
  );
  return {
    schemaVersion: 1,
    includeFormal,
    totalTargets: targets.length,
    targets,
  };
}
