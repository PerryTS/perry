import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  await new Promise<void>((resolve, reject) =>
    session.post(
      "Debugger.enable",
      { maxScriptsCacheSize: 1_000_000 },
      (err, value) => {
        if (err) return reject(err);
        console.log(
          "debugger id:",
          typeof value.debuggerId,
          value.debuggerId.length > 0,
          Object.keys(value).sort().join(","),
        );
        resolve();
      },
    )
  );
  await new Promise<void>((resolve) =>
    session.post("Debugger.disable", (err, value) => {
      console.log("disable:", err === null, Object.keys(value).length);
      resolve();
    })
  );
} finally {
  session.disconnect();
}
