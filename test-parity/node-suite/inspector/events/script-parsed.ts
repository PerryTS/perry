import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  const scripts: any[] = [];
  session.on("Debugger.scriptParsed", ({ params }) => scripts.push(params));
  await new Promise<void>((resolve, reject) =>
    session.post("Debugger.enable", (err) => err ? reject(err) : resolve())
  );
  await new Promise<void>((resolve, reject) =>
    session.post("Runtime.evaluate", {
      expression: "1 + 2\n//# sourceURL=inspector-parity-marker.js",
    }, (err) => err ? reject(err) : resolve())
  );
  const script = scripts.find((item) =>
    item.url === "inspector-parity-marker.js"
  );
  console.log("found:", Boolean(script));
  console.log(
    "shape:",
    typeof script.scriptId,
    script.url,
    typeof script.startLine,
    typeof script.startColumn,
    typeof script.endLine,
    typeof script.endColumn,
    typeof script.executionContextId,
    script.isLiveEdit,
    script.sourceMapURL === "",
  );
} finally {
  session.removeAllListeners();
  session.disconnect();
}
