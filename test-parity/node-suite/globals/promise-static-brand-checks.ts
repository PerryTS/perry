function formatValue(value: any): string {
  if (Array.isArray(value)) {
    return JSON.stringify(value);
  }
  return String(value);
}

async function showPromise(label: string, promise: Promise<any>) {
  try {
    console.log(label + ":fulfilled:" + formatValue(await promise));
  } catch (error: any) {
    console.log(label + ":rejected:" + formatValue(error));
  }
}

function showThrow(label: string, fn: () => any) {
  try {
    const result = fn();
    console.log(label + ":ok:" + formatValue(result));
  } catch (error: any) {
    console.log(label + ":throw:" + error.name);
  }
}

console.log("typeof resolve:", typeof Promise.resolve);
console.log("resolve length:", Promise.resolve.length);
console.log("all length:", Promise.all.length);

const detachedResolve = Promise.resolve;
console.log("detached resolve typeof:", typeof detachedResolve);
console.log("detached resolve length:", detachedResolve.length);

await showPromise("resolve.call.Promise", Promise.resolve.call(Promise, 7));
await showPromise("reject.call.Promise", Promise.reject.call(Promise, "bad"));
await showPromise("all.call.Promise", Promise.all.call(Promise, [Promise.resolve(1), 2]));
await showPromise("race.call.Promise", Promise.race.call(Promise, [Promise.resolve("race")]));
await showPromise(
  "allSettled.call.Promise",
  Promise.allSettled.call(Promise, [Promise.resolve(1), Promise.reject("err")]),
);
await showPromise("any.call.Promise", Promise.any.call(Promise, [Promise.reject("x"), "ok"]));

showThrow("resolve.detached", () => {
  return detachedResolve(1);
});
showThrow("resolve.call.object", () => Promise.resolve.call({} as any, 1));
showThrow("reject.call.object", () => Promise.reject.call({} as any, "x"));
showThrow("all.call.object", () => Promise.all.call({} as any, []));
showThrow("race.call.object", () => Promise.race.call({} as any, []));
showThrow("allSettled.call.object", () => Promise.allSettled.call({} as any, []));
showThrow("any.call.object", () => Promise.any.call({} as any, []));
showThrow("resolve.apply.object", () => Promise.resolve.apply({} as any, [1]));
showThrow("all.apply.object", () => Promise.all.apply({} as any, [[]]));
showThrow("resolve.bind.object", () => Promise.resolve.bind({} as any)(1));
