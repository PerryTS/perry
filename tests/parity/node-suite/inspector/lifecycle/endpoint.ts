import { spawnSync } from "node:child_process";
import inspector from "node:inspector";

function reportSync(label: string, fn: () => unknown): boolean {
  try {
    const value = fn();
    console.log(
      label,
      "ok",
      value === undefined ? "undefined" : typeof value,
    );
    return true;
  } catch (err) {
    const error = err as {
      constructor?: { name?: string };
      code?: string;
      message?: string;
    };
    console.log(
      label,
      "err",
      error.constructor?.name,
      error.code,
      String(error.message).split("\n")[0],
    );
    return false;
  }
}

if (process.env.PERRY_INSPECTOR_ENDPOINT_CHILD === "1") {
  let opened = false;
  try {
    console.log(
      "surface:",
      typeof inspector.open,
      typeof inspector.close,
      typeof inspector.url,
      typeof inspector.waitForDebugger,
      typeof inspector.console,
      typeof inspector.Session,
    );
    console.log("url before:", inspector.url() === undefined);
    console.log(
      "url receiver ignored:",
      Reflect.apply(inspector.url, null, [1, 2]) === undefined,
    );
    console.log(
      "close receiver ignored:",
      Reflect.apply(inspector.close, null, [1, 2]) === undefined,
    );
    reportSync("wait inactive:", () => inspector.waitForDebugger());

    const firstHandle = inspector.open(0, "127.0.0.1", false) as any;
    opened = true;
    const firstUrl = inspector.url();
    console.log(
      "open handle:",
      typeof firstHandle,
      typeof firstHandle[Symbol.dispose],
    );
    console.log(
      "url active:",
      typeof firstUrl,
      /^ws:\/\/127\.0\.0\.1:\d+\/[0-9a-f-]+$/i.test(String(firstUrl)),
    );
    firstHandle[Symbol.dispose]();
    opened = false;
    console.log("url after dispose:", inspector.url() === undefined);

    inspector.open(0, "127.0.0.1", false);
    opened = true;
    console.log("url reopened:", typeof inspector.url());
    if (reportSync("close:", () => inspector.close())) opened = false;
    console.log("url after close:", inspector.url() === undefined);
    console.log(
      "console object:",
      typeof inspector.console,
      inspector.console !== null,
    );
  } finally {
    if (opened) inspector.close();
  }
} else {
  const child = spawnSync(process.execPath, [import.meta.filename], {
    encoding: "utf8",
    env: { ...process.env, PERRY_INSPECTOR_ENDPOINT_CHILD: "1" },
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
