import inspector, { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  await new Promise<void>((resolve) =>
    session.post("Runtime.enable", () => resolve())
  );
  const notification = new Promise<any>((resolve) =>
    session.once("Runtime.consoleAPICalled", resolve)
  );
  inspector.console.warn("marker", 42, true);
  const { method, params } = await notification;
  console.log(
    "event:",
    method,
    params.type,
    params.args.map((arg: any) => `${arg.type}:${arg.value}`).join(","),
  );
  console.log(
    "context:",
    typeof params.executionContextId,
    typeof params.timestamp,
    Array.isArray(params.stackTrace.callFrames),
  );
} finally {
  session.disconnect();
}
