import inspector from "node:inspector";

function reportSync(label: string, fn: () => unknown): void {
  try {
    const value = fn();
    console.log(label, "ok", value === undefined ? "undefined" : typeof value);
  } catch (err) {
    const error = err as { constructor?: { name?: string }, code?: string, message?: string };
    console.log(label, "err", error.constructor?.name, error.code, String(error.message).split("\n")[0]);
  }
}

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
reportSync("wait inactive:", () => inspector.waitForDebugger());

const firstHandle = inspector.open(0, "127.0.0.1", false) as any;
const firstUrl = inspector.url();
console.log("open handle:", typeof firstHandle, typeof firstHandle[Symbol.dispose]);
console.log(
  "url active:",
  typeof firstUrl,
  /^ws:\/\/127\.0\.0\.1:\d+\/[0-9a-f-]+$/i.test(String(firstUrl)),
);
firstHandle[Symbol.dispose]();
console.log("url after dispose:", inspector.url() === undefined);

inspector.open(0, "127.0.0.1", false);
console.log("url reopened:", typeof inspector.url());
reportSync("close:", () => inspector.close());
console.log("url after close:", inspector.url() === undefined);
console.log("console object:", typeof inspector.console, inspector.console !== null);
