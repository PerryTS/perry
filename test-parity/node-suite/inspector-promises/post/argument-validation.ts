import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  const cases: Array<[string, () => Promise<unknown>]> = [
    ["missing", () => session.post()],
    ["number method", () => session.post(1 as never)],
    ["number params", () => session.post("Runtime.enable", 1 as never)],
    ["string params", () => session.post("Runtime.enable", "x" as never)],
    [
      "function params",
      () => session.post("Runtime.enable", (() => {}) as never),
    ],
  ];
  for (const [label, run] of cases) {
    let pending: Promise<unknown> | undefined;
    let synchronous = false;
    try {
      pending = run();
    } catch {
      synchronous = true;
    }
    try {
      await pending;
      console.log(label, "unexpected", synchronous);
    } catch (error) {
      const cause = error as { name?: string; code?: string };
      console.log(
        label,
        synchronous,
        pending instanceof Promise,
        cause.name,
        cause.code,
      );
    }
  }
} finally {
  session.disconnect();
}
