// #10521: the resolving functions a Promise hands out (executor, thenable job,
// combinator elements, the GetCapabilitiesExecutor) carry `length`, `name` ""
// and no [[Construct]] as facts of their function kind, not as per-closure
// side-table entries. Reflection must still see exactly what Node sees, and a
// user redefinition or delete of either property must still land on the one
// closure it targeted.

function describe(label: string, f: any): void {
  let ctor = "constructs";
  try {
    new f();
  } catch (e) {
    ctor = e instanceof TypeError ? "TypeError" : "other";
  }
  console.log(
    label,
    typeof f,
    JSON.stringify(Object.getOwnPropertyDescriptor(f, "length")),
    JSON.stringify(Object.getOwnPropertyDescriptor(f, "name")),
    JSON.stringify(Object.getOwnPropertyNames(f).sort()),
    Object.prototype.hasOwnProperty.call(f, "prototype"),
    ctor,
  );
}

async function main(): Promise<void> {
  // new Promise(executor)
  let res: any;
  let rej: any;
  const p = new Promise((resolve, reject) => {
    res = resolve;
    rej = reject;
  });
  describe("executor.resolve", res);
  describe("executor.reject", rej);
  res(1);
  console.log("settled", await p);

  // A second pair is independent of the first.
  let res2: any;
  new Promise((resolve) => {
    res2 = resolve;
  });
  console.log("distinct", res !== res2, res2.length, res2.name === "");

  // Promise subclass construction.
  class Sub extends Promise<number> {}
  let subRes: any;
  const s = new Sub((resolve) => {
    subRes = resolve;
  });
  describe("subclass.resolve", subRes);
  subRes(2);
  console.log("subclass settled", await s);

  // The thenable job's resolving functions.
  let jobRes: any;
  let jobRej: any;
  const thenable = {
    then(onFulfilled: any, onRejected: any) {
      jobRes = onFulfilled;
      jobRej = onRejected;
      onFulfilled(3);
    },
  };
  console.log("thenable settled", await Promise.resolve(thenable));
  describe("thenable.resolve", jobRes);
  describe("thenable.reject", jobRej);

  // Promise.all element functions and the GetCapabilitiesExecutor.
  let capExecutor: any;
  let element: any;
  function NotPromise(executor: any) {
    capExecutor = executor;
    executor(
      () => {},
      () => {},
    );
  }
  (NotPromise as any).resolve = (v: any) => v;
  const elementThenable = {
    then(onFulfilled: any) {
      element = onFulfilled;
    },
  };
  Promise.all.call(NotPromise, [elementThenable]);
  describe("all.element", element);
  describe("capability.executor", capExecutor);

  // A redefinition / delete targets exactly one closure.
  let a: any;
  let b: any;
  new Promise((resolve, reject) => {
    a = resolve;
    b = reject;
  });
  Object.defineProperty(a, "name", { value: "renamed" });
  delete b.length;
  console.log("redefined", a.name, JSON.stringify(Object.getOwnPropertyDescriptor(a, "name")));
  console.log("redefined keys", JSON.stringify(Object.keys(a)));
  console.log(
    "deleted",
    Object.prototype.hasOwnProperty.call(b, "length"),
    JSON.stringify(Object.getOwnPropertyNames(b)),
  );
  console.log("untouched", res.name === "", res.length, rej.name === "", rej.length);
}

main();
