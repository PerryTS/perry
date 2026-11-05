import { Worker } from "node:worker_threads";

const dispose = Worker.prototype[Symbol.asyncDispose];
console.log("method:", typeof dispose);
if (typeof dispose === "function") {
  const descriptor = Object.getOwnPropertyDescriptor(Worker.prototype, Symbol.asyncDispose)!;
  console.log("descriptor:", descriptor.writable, descriptor.enumerable, descriptor.configurable);
  console.log("function:", dispose.name, dispose.length,
    Object.getPrototypeOf(dispose) === Object.getPrototypeOf(async function () {}),
    Object.hasOwn(dispose, "prototype"));
  try {
    Reflect.construct(dispose, []);
  } catch (error) {
    console.log("construct:", error instanceof TypeError);
  }

  for (const kind of ["primitive", "promise", "thenable", "reject", "then-reject",
    "get-throw", "call-throw", "then-throw", "constructor-throw", "own-then", "own-then-reject", "inherited-then"]) {
    const sentinel = new Error(kind);
    const trace: string[] = [];
    let receiver: any;
    let returned: any = 42;
    if (kind === "promise" || kind === "own-then" || kind === "own-then-reject" || kind === "constructor-throw" || kind === "inherited-then") {
      returned = kind === "own-then-reject" ? Promise.reject(sentinel) : Promise.resolve(42);
      if (kind === "constructor-throw") {
        Object.defineProperty(returned, "constructor", { get() { throw sentinel; } });
      } else if (kind === "inherited-then") {
        Object.setPrototypeOf(returned, Object.create(Promise.prototype, {
          then: { get() { trace.push("unexpected then"); throw sentinel; } },
        }));
      } else if (kind !== "promise") {
        Object.defineProperty(returned, "then", { get() { trace.push("unexpected then"); throw sentinel; } });
      }
    }
    if (kind === "reject") returned = Promise.reject(sentinel);
    if (kind === "thenable" || kind === "then-reject" || kind === "then-throw") {
      returned = Object.defineProperty({}, "then", { get() {
        trace.push("then get");
        if (kind === "then-throw") throw sentinel;
        return function (resolve: any, reject: any) {
          trace.push("then call:" + (this === returned));
          if (kind === "then-reject") reject(sentinel);
          else resolve(42);
        };
      } });
    }
    receiver = Object.defineProperty({}, "terminate", { get() {
      trace.push("get:" + (this === receiver));
      if (kind === "get-throw") throw sentinel;
      return function (...args: any[]) {
        trace.push("call:" + (this === receiver) + ":" + args.length);
        if (kind === "call-throw") throw sentinel;
        return returned;
      };
    } });
    try {
      const result = dispose.call(receiver);
      trace.push("returned:" + (result instanceof Promise));
      console.log(kind, "fulfilled", await result === undefined, trace.join(","));
    } catch (error) {
      console.log(kind, "rejected", error === sentinel, trace.join(","));
    }
  }

  for (const kind of ["custom-constructor-fulfill", "custom-constructor-get-throw"]) {
    const sentinel = new Error(kind);
    const trace: string[] = [];
    const returned = Promise.resolve("original");
    Object.defineProperty(returned, "constructor", { value: function Other() {} });
    Object.defineProperty(returned, "then", { get() {
      trace.push("then get");
      if (kind === "custom-constructor-get-throw") throw sentinel;
      return function (resolve: any) {
        trace.push("then call:" + (this === returned));
        resolve("replacement");
      };
    } });
    try {
      const result = dispose.call({ terminate() { return returned; } });
      trace.push("returned");
      console.log(kind, "fulfilled", await result === undefined, trace.join(","));
    } catch (error) {
      console.log(kind, "rejected", error === sentinel, trace.join(","));
    }
  }

  for (const receiver of [{}, { terminate: undefined }, { terminate: null },
    { terminate: 0 }, { terminate: 1 }, null, undefined, 1]) {
    let synchronous = true;
    try {
      const result = dispose.call(receiver);
      synchronous = false;
      await result;
    } catch (error: any) {
      console.log("invalid:", synchronous, error.name, error.code, error.message);
    }
  }

  for (const kind of ["derived", "proxy"]) {
    let receiver: any;
    const base = { terminate(...args: any[]) {
      console.log(kind, this === receiver, args.length);
      return Promise.resolve(123);
    } };
    receiver = kind === "derived" ? Object.create(base) : new Proxy(base, {});
    console.log(kind + " result:", await dispose.call(receiver));
  }
}
