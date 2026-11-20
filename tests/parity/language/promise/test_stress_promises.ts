// Stress test: Promise correctness
// Targets: Promise combinators, error propagation, chaining, microtask ordering
// Based on bugs: Promise.race rejection, Promise.any, Promise.withResolvers

// === SECTION: Basic Promise.resolve/reject ===
const p1 = Promise.resolve(42);
p1.then((v: number) => console.log("resolve:", v));

const p2 = Promise.reject("error");
p2.catch((e: any) => console.log("reject:", e));

// === SECTION: Promise chaining ===
Promise.resolve(1)
  .then((v: number) => v + 1)
  .then((v: number) => v * 3)
  .then((v: number) => console.log("chain:", v));

// === SECTION: Promise.then with two args ===
Promise.reject("err")
  .then(
    (v: any) => console.log("should not run"),
    (e: any) => console.log("onRejected:", e)
  );

// === SECTION: Promise.catch returns new promise ===
Promise.reject("fail")
  .catch((e: any) => "recovered: " + e)
  .then((v: any) => console.log("recovered:", v));

// === SECTION: Promise.finally ===
let finallyRan = false;
Promise.resolve("done")
  .finally(() => { finallyRan = true; })
  .then((v: any) => console.log("finally value:", v, "ran:", finallyRan));

// === SECTION: Promise.all - all resolve ===
Promise.all([
  Promise.resolve(1),
  Promise.resolve(2),
  Promise.resolve(3),
]).then((values: number[]) => console.log("all:", values));

// === SECTION: Promise.all - one rejects ===
Promise.all([
  Promise.resolve(1),
  Promise.reject("fail"),
  Promise.resolve(3),
]).catch((e: any) => console.log("all reject:", e));

// === SECTION: Promise.all - empty array ===
Promise.all([]).then((values: any[]) => console.log("all empty:", values));

// === SECTION: Promise.race - first resolves ===
Promise.race([
  Promise.resolve("first"),
  Promise.resolve("second"),
]).then((v: any) => console.log("race resolve:", v));

// === SECTION: Promise.race - first rejects ===
Promise.race([
  Promise.reject("race-err"),
  Promise.resolve("late"),
]).catch((e: any) => console.log("race reject:", e));

// === SECTION: Promise.any - first success ===
Promise.any([
  Promise.reject("err1"),
  Promise.resolve("success"),
  Promise.reject("err3"),
]).then((v: any) => console.log("any:", v));

// === SECTION: Promise.any - all reject ===
Promise.any([
  Promise.reject("a"),
  Promise.reject("b"),
  Promise.reject("c"),
]).catch((e: any) => {
  console.log("any all rejected:", e instanceof AggregateError);
  console.log("any errors length:", (e as AggregateError).errors.length);
});

// === SECTION: Nested then chains ===
Promise.resolve(1)
  .then((v: number) => {
    return Promise.resolve(v + 10);
  })
  .then((v: number) => {
    return Promise.resolve(v + 100);
  })
  .then((v: number) => console.log("nested chain:", v));

// === SECTION: Error in then handler ===
Promise.resolve(1)
  .then((v: number) => {
    throw new Error("in-then");
  })
  .catch((e: any) => console.log("catch thrown:", e.message));

// === SECTION: Multiple catch handlers ===
Promise.reject("original")
  .catch((e: any) => {
    throw new Error("from-catch");
  })
  .catch((e: any) => console.log("double catch:", e.message));

// === SECTION: Promise constructor ===
const p3 = new Promise<number>((resolve, reject) => {
  resolve(99);
});
p3.then((v: number) => console.log("constructor resolve:", v));

const p4 = new Promise<number>((resolve, reject) => {
  reject("constructor-err");
});
p4.catch((e: any) => console.log("constructor reject:", e));

// === SECTION: Values in different promise states ===
Promise.resolve(undefined).then((v: any) => console.log("resolve undefined:", v));
Promise.resolve(null).then((v: any) => console.log("resolve null:", v));
Promise.resolve(0).then((v: any) => console.log("resolve 0:", v));
Promise.resolve("").then((v: any) => console.log("resolve empty:", v));
Promise.resolve(false).then((v: any) => console.log("resolve false:", v));
Promise.resolve([1, 2]).then((v: any) => console.log("resolve array:", v));
