import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { resolveAcceptanceCoordinate } from "./engine-acceptance-coordinate.mjs";

function parseArguments(arguments_) {
  const values = new Map();
  for (let index = 0; index < arguments_.length; index += 2) {
    const key = arguments_[index];
    const value = arguments_[index + 1];
    if (!key?.startsWith("--") || value === undefined || value.startsWith("--")) {
      throw new Error("expected --engine, --target-platform and --accelerator values");
    }
    if (values.has(key)) {
      throw new Error(`duplicate argument ${key}`);
    }
    values.set(key, value);
  }
  const allowed = new Set(["--engine", "--target-platform", "--accelerator"]);
  for (const key of values.keys()) {
    if (!allowed.has(key)) {
      throw new Error(`unknown argument ${key}`);
    }
  }
  for (const key of allowed) {
    if (!values.has(key)) {
      throw new Error(`missing argument ${key}`);
    }
  }
  return {
    engine: values.get("--engine"),
    targetPlatform: values.get("--target-platform"),
    accelerator: values.get("--accelerator"),
  };
}

try {
  const matrix = JSON.parse(
    readFileSync(resolve("contracts/inference-engines/v1-support-matrix.json"), "utf8"),
  );
  const coordinate = resolveAcceptanceCoordinate(matrix, parseArguments(process.argv.slice(2)));
  process.stdout.write(`${JSON.stringify(coordinate)}\n`);
} catch (error) {
  const message = error instanceof Error ? error.message : "unknown coordinate validation error";
  process.stderr.write(`HAL100 acceptance coordinate rejected: ${message}\n`);
  process.exitCode = 2;
}
