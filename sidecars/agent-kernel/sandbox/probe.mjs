import { spawn } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { createConnection } from "node:net";

const [readPath, writePath, rawPort] = process.argv.slice(2);
if (!readPath || !writePath || !rawPort) {
  process.exitCode = 64;
} else {
  const readDenied = await operationIsDenied(() => readFile(readPath, "utf8"));
  const writeDenied = await operationIsDenied(() => writeFile(writePath, "unexpected"));
  const networkDenied = await networkIsDenied(Number(rawPort));
  const processDenied = await processIsDenied();
  const inheritedEnvironmentAbsent =
    process.env.PATH === undefined &&
    process.env.SSH_AUTH_SOCK === undefined &&
    process.env.HTTP_PROXY === undefined &&
    process.env.HTTPS_PROXY === undefined;
  const isolatedDirectories =
    process.env.HOME?.endsWith("/home") === true && process.env.TMPDIR?.endsWith("/tmp") === true;

  const result = {
    readDenied,
    writeDenied,
    networkDenied,
    processDenied,
    inheritedEnvironmentAbsent,
    isolatedDirectories,
  };
  process.stdout.write(`${JSON.stringify(result)}\n`);

  if (Object.values(result).some((value) => !value)) {
    process.exitCode = 1;
  }
}

async function operationIsDenied(operation) {
  try {
    await operation();
    return false;
  } catch (error) {
    return error?.code === "EPERM" || error?.code === "EACCES";
  }
}

async function networkIsDenied(port) {
  return new Promise((resolve) => {
    const socket = createConnection({ host: "127.0.0.1", port });
    const timeout = setTimeout(() => {
      socket.destroy();
      resolve(false);
    }, 1_000);

    socket.once("connect", () => {
      clearTimeout(timeout);
      socket.destroy();
      resolve(false);
    });
    socket.once("error", (error) => {
      clearTimeout(timeout);
      resolve(error?.code === "EPERM" || error?.code === "EACCES");
    });
  });
}

async function processIsDenied() {
  return new Promise((resolve) => {
    let child;
    try {
      child = spawn("/bin/echo", ["unexpected"]);
    } catch (error) {
      resolve(error?.code === "EPERM" || error?.code === "EACCES");
      return;
    }
    const timeout = setTimeout(() => {
      child.kill();
      resolve(false);
    }, 1_000);

    child.once("error", (error) => {
      clearTimeout(timeout);
      resolve(error?.code === "EPERM" || error?.code === "EACCES");
    });
    child.once("exit", () => {
      clearTimeout(timeout);
      resolve(false);
    });
  });
}
