import { spawnSync } from "node:child_process";
import inspector from "node:inspector";

if (process.env.PERRY_INSPECTOR_RANGE_CHILD === "1") {
  try {
    for (const port of [65536, 2 ** 32 - 1]) {
      try {
        inspector.open(port);
        console.log(port, "unexpected");
      } catch (cause) {
        const error = cause as {
          name?: string;
          code?: string;
          message?: string;
        };
        console.log(
          port,
          error.name,
          error.code,
          error.message?.includes(">= 0 && <= 65535"),
        );
      } finally {
        inspector.close();
      }
    }
  } finally {
    inspector.close();
  }
} else {
  const child = spawnSync(process.execPath, [import.meta.filename], {
    encoding: "utf8",
    env: { ...process.env, PERRY_INSPECTOR_RANGE_CHILD: "1" },
    timeout: 5_000,
  });
  if (child.error) {
    const error = child.error as { code?: string };
    console.log("child error:", error.code ?? error.constructor.name);
  } else {
    process.stdout.write(child.stdout);
    console.log("child exit:", child.status, child.signal ?? "none");
  }
}
