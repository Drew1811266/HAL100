import { validateAcceptanceEnvironment } from "./engine-acceptance-coordinate.mjs";

function parseEngine(arguments_) {
  if (arguments_.length !== 2 || arguments_[0] !== "--engine" || !arguments_[1]) {
    throw new Error(
      "usage: node scripts/validate-engine-acceptance-environment.mjs --engine ENGINE",
    );
  }
  return arguments_[1];
}

try {
  const report = validateAcceptanceEnvironment(parseEngine(process.argv.slice(2)), process.env);
  process.stdout.write(`${JSON.stringify(report)}\n`);
} catch (error) {
  const message = error instanceof Error ? error.message : "unknown acceptance environment error";
  process.stderr.write(`HAL100 acceptance environment rejected: ${message}\n`);
  process.exitCode = 2;
}
