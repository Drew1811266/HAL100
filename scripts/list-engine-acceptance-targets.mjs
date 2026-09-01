import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { buildAcceptanceTargets } from "./engine-acceptance-coordinate.mjs";

const arguments_ = process.argv.slice(2);
if (arguments_.some((argument) => argument === "--help" || argument === "-h")) {
  process.stdout.write(
    "usage: node scripts/list-engine-acceptance-targets.mjs [--all]\n" +
      "Lists pending support cells by default; --all also includes already formal external cells.\n",
  );
} else {
  try {
    if (arguments_.some((argument) => argument !== "--all") || arguments_.length > 1) {
      throw new Error("only --all is supported");
    }
    const matrix = JSON.parse(
      readFileSync(resolve("contracts/inference-engines/v1-support-matrix.json"), "utf8"),
    );
    const report = buildAcceptanceTargets(matrix, { includeFormal: arguments_[0] === "--all" });
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } catch (error) {
    const message = error instanceof Error ? error.message : "unknown target inventory error";
    process.stderr.write(`HAL100 acceptance target inventory failed: ${message}\n`);
    process.exitCode = 2;
  }
}
