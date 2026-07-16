import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  const parsed = new Promise<any>((resolve) => {
    const listener = (message: any) => {
      if (message.params.url === "inspector-source-marker.js") {
        session.off("Debugger.scriptParsed", listener);
        resolve(message.params);
      }
    };
    session.on("Debugger.scriptParsed", listener);
  });
  await new Promise<void>((resolve, reject) =>
    session.post("Debugger.enable", (err) => err ? reject(err) : resolve())
  );
  const source =
    "globalThis.__inspectorSourceMarker = 42;\n//# sourceURL=inspector-source-marker.js";
  await new Promise<void>((resolve, reject) =>
    session.post(
      "Runtime.evaluate",
      { expression: source },
      (err) => err ? reject(err) : resolve(),
    )
  );
  const { scriptId } = await parsed;
  await new Promise<void>((resolve, reject) =>
    session.post("Debugger.getScriptSource", { scriptId }, (err, value) => {
      if (err) return reject(err);
      console.log(
        "source:",
        value.scriptSource === source,
        typeof value.bytecode === "undefined",
      );
      resolve();
    })
  );
} finally {
  session.removeAllListeners();
  session.disconnect();
}
