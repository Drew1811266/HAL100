import { installStartupRecovery } from "./startup-recovery";

const startup = installStartupRecovery();

void import("./main")
  .then(({ mountApplication }) => {
    mountApplication(startup.markReady);
  })
  .catch(() => startup.fail("module_load_failed"));
