import inspector, { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  let persistent = 0;
  let once = 0;
  const listener = () => persistent++;
  session.on("Runtime.consoleAPICalled", listener);
  session.once("Runtime.consoleAPICalled", () => once++);
  await new Promise<void>((resolve) =>
    session.post("Runtime.enable", () => resolve())
  );
  inspector.console.log(1);
  inspector.console.log(2);
  await new Promise((resolve) => setImmediate(resolve));
  session.off("Runtime.consoleAPICalled", listener);
  inspector.console.log(3);
  await new Promise((resolve) => setImmediate(resolve));
  console.log(
    "counts:",
    persistent,
    once,
    session.listenerCount("Runtime.consoleAPICalled"),
  );
} finally {
  session.removeAllListeners();
  session.disconnect();
}
