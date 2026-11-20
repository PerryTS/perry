async function main() {
  const events: string[] = [];

  const doubleResolve = {
    then(resolve: (value: string) => void) {
      events.push("then");
      resolve("ok");
      resolve("bad");
    },
  };

  const allPromise = Promise.all([doubleResolve as any, 2]);
  allPromise.then(() => events.push("all-then"));
  events.push("after-call");

  const allValues = await allPromise;
  console.log("all:", allValues.join(","), events.join(">"));

  const originalArrayThen = (Array.prototype as any).then;
  try {
    (Array.prototype as any).then = function (resolve: (value: string) => void) {
      resolve("array-assimilated");
    };
    console.log("all-array-then:", await Promise.all([]));
  } finally {
    if (originalArrayThen === undefined) {
      delete (Array.prototype as any).then;
    } else {
      (Array.prototype as any).then = originalArrayThen;
    }
  }

  try {
    await Promise.all([
      {
        then(_resolve: (value: string) => void, reject: (reason: string) => void) {
          reject("boom");
        },
      } as any,
    ]);
    console.log("all-reject: missing");
  } catch (e) {
    console.log("all-reject:", e);
  }

  const settled = await Promise.allSettled([
    {
      then(_resolve: (value: string) => void, reject: (reason: string) => void) {
        reject("no");
      },
    } as any,
    {
      then(resolve: (value: string) => void) {
        resolve("yes");
      },
    } as any,
  ]);

  const labels = settled.map((result: any) => {
    if (result.status === "fulfilled") {
      return result.status + ":" + result.value;
    }
    return result.status + ":" + result.reason;
  });
  console.log("settled:", labels.join(","));
}

main();
