import inspector from "node:inspector";

function reportSync(label: string, fn: () => unknown): void {
  try {
    const value = fn();
    console.log(label, "ok", value === undefined ? "undefined" : typeof value);
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
  }
}

const session = new inspector.Session();
console.log(
  "surface:",
  typeof session.connect,
  typeof session.connectToMainThread,
  typeof session.disconnect,
  typeof session.post,
  typeof session.on,
  typeof session.once,
);

reportSync(
  "post before connect:",
  () => session.post("Runtime.evaluate", {}, () => {}),
);

try {
  reportSync("connectToMainThread:", () => session.connectToMainThread());
  reportSync("connect:", () => session.connect());

  await new Promise<void>((resolve) => {
    session.post(
      "Runtime.evaluate",
      { expression: "1 + 2", returnByValue: true },
      (err, result) => {
        console.log(
          "eval callback:",
          err === null,
          result?.result?.type,
          result?.result?.value,
          result?.result?.description,
        );
        resolve();
      },
    );
  });

  await new Promise<void>((resolve) => {
    session.post("Nope.nope", {}, (err, result) => {
      const error = err as {
        constructor?: { name?: string };
        code?: string;
        message?: string;
      } | null;
      console.log(
        "bad callback:",
        error?.constructor?.name ?? "<none>",
        error?.code ?? "<none>",
        String(error?.message).includes("wasn't found"),
        result === undefined,
      );
      resolve();
    });
  });

  let genericCount = 0;
  let specificCount = 0;
  let firstGeneric = "";
  let firstSpecific = "";
  session.on("inspectorNotification", (message: { method?: string }) => {
    genericCount++;
    firstGeneric ||= message?.method || "";
  });
  session.on("Runtime.consoleAPICalled", (message: { method?: string }) => {
    specificCount++;
    firstSpecific ||= message?.method || "";
  });

  await new Promise<void>((resolve) => {
    session.post("Runtime.enable", {}, (err, result) => {
      console.log(
        "enable callback:",
        err === null,
        Object.keys(result || {}).length,
      );
      resolve();
    });
  });
  const notification = new Promise<void>((resolve) => {
    session.once("Runtime.consoleAPICalled", () => resolve());
  });
  await new Promise<void>((resolve) => {
    session.post("Runtime.evaluate", {
      expression: 'console.log("callback-session-event")',
    }, () => resolve());
  });
  await notification;
  console.log(
    "events:",
    genericCount > 0,
    specificCount > 0,
    firstGeneric,
    firstSpecific,
  );

  reportSync("disconnect:", () => session.disconnect());
  reportSync(
    "post after disconnect:",
    () => session.post("Runtime.evaluate", {}, () => {}),
  );
} finally {
  session.removeAllListeners();
  session.disconnect();
}
