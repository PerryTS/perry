// #11919 item 3b: async_hooks and domain values are ordinary GC objects
// owning native payloads. Shape, identity, lifecycle reuse and churn match
// Node. Node deliberately keeps disable()/emitDestroy() reusable; the one
// receiver error below is the family's actual closed/invalid-state analogue.
import {
  AsyncLocalStorage,
  AsyncResource,
  createHook,
} from "async_hooks";
import * as domain from "domain";

function shape(label: string, value: any, ctor: any): void {
  console.log(
    label,
    typeof value,
    value instanceof ctor,
    JSON.stringify(Object.keys(value)),
    JSON.stringify(value),
    value.constructor.name,
    JSON.stringify(Object.getOwnPropertyNames(Object.getPrototypeOf(value)).sort()),
  );
}

const als1 = new AsyncLocalStorage<number>();
const als2 = new AsyncLocalStorage<number>();
const resource1 = new AsyncResource("payload-test");
const resource2 = new AsyncResource("payload-test");
const hook1 = createHook({});
const hook2 = createHook({});
const domain1 = domain.create();
const domain2 = domain.create();

shape("als", als1, AsyncLocalStorage);
shape("resource", resource1, AsyncResource);
shape("hook", hook1, hook1.constructor);
shape("domain", domain1, domain.Domain);

console.log(
  "identity",
  als1 !== als2,
  resource1 !== resource2,
  hook1 !== hook2,
  domain1 !== domain2,
);

const map = new Map<any, string>([
  [als1, "als"],
  [resource1, "resource"],
  [hook1, "hook"],
  [domain1, "domain"],
]);
const set = new Set<any>([als1, als2, resource1, resource2, hook1, hook2, domain1, domain2]);
const weak = new WeakMap<object, number>([
  [als1, 1],
  [resource1, 2],
  [hook1, 3],
  [domain1, 4],
]);
console.log("keys", map.size, set.size, map.get(hook1), weak.get(domain1));

als1.disable();
resource1.emitDestroy();
hook1.disable();
console.log(
  "reuse",
  als1.getStore(),
  als1.run(3, () => als1.getStore()),
  resource1.runInAsyncScope(() => 7),
  resource1.asyncId() > 0,
  hook1.enable() === hook1,
  hook1.disable() === hook1,
);
console.log("disable inside run", als1.run(5, () => {
  als1.disable();
  return als1.getStore();
}));

let seen = 0;
let nesting = false;
const reentrant = createHook({
  init(_id: number, type: string) {
    if (type !== "payload-reentrant") return;
    seen++;
    if (!nesting) {
      nesting = true;
      reentrant.disable();
      new AsyncResource("payload-reentrant").emitDestroy();
      reentrant.enable();
    }
  },
}).enable();
new AsyncResource("payload-reentrant").emitDestroy();
new AsyncResource("payload-reentrant").emitDestroy();
reentrant.disable();
console.log("nested hook reuse", seen);

let order = "";
let validIds = true;
const first = createHook({ init(id: number, type: string, _trigger: number, resource: any) {
  if (type === "payload-order") { order += "a"; validIds = validIds && resource.asyncId() === id; }
}}).enable();
const second = createHook({ init(_id: number, type: string) {
  if (type === "payload-order") order += "b";
}}).enable();
first.disable();
first.enable();
new AsyncResource("payload-order").emitDestroy();
first.disable();
second.disable();
console.log("hook enable order", order, validIds);

let scoped = false;
domain1.run(() => {
  const objects: any[] = [];
  for (let i = 0; i < 2_000; i++) objects.push({ i });
  scoped = objects.length === 2_000;
});
const member = {};
domain1.add(member);
console.log("domain reuse", scoped, domain1.remove(member) === domain1, domain1.members.length);

try {
  AsyncLocalStorage.prototype.getStore.call({});
} catch (error: any) {
  console.log("invalid receiver", error.constructor.name, error.code);
}

let churn = 0;
let warmRss = 0;
for (let i = 0; i < 50_000; i++) {
  if (i === 10_000) warmRss = process.memoryUsage().rss;
  switch (i & 3) {
    case 0: {
      const value = new AsyncLocalStorage();
      value.disable();
      churn += value instanceof AsyncLocalStorage ? 1 : 0;
      break;
    }
    case 1: {
      const value = new AsyncResource("churn");
      value.emitDestroy();
      churn += value instanceof AsyncResource ? 1 : 0;
      break;
    }
    case 2: {
      const value = createHook({}).enable();
      value.disable();
      churn += typeof value === "object" ? 1 : 0;
      break;
    }
    default: {
      const value = domain.create();
      value.exit();
      churn += value instanceof domain.Domain ? 1 : 0;
      break;
    }
  }
}
if (process.memoryUsage().rss - warmRss > 32 * 1024 * 1024) {
  throw new Error("native-payload churn exceeded its RSS bound");
}
console.log("churn", churn);
