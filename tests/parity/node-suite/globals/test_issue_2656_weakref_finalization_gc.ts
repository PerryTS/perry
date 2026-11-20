declare function gc(): void;

function churn(count: number): void {
  let junk: any[] = [];
  for (let i = 0; i < count; i++) {
    junk.push({ i, payload: "weakref-gc-" + i });
    if (junk.length > 64) {
      junk = [];
    }
  }
}

const cleanup: string[] = [];
const registry = new FinalizationRegistry((held: string) => {
  cleanup.push(held);
});

let weak = new WeakRef({ marker: "placeholder" });
let removedWeak = new WeakRef({ marker: "placeholder" });

(function setup() {
  let target: any = { marker: "target" };
  churn(20_000);
  weak = new WeakRef(target);
  registry.register(target, "held-value");
  console.log("initial deref:", weak.deref().marker);
  target = null;

  const token = { token: true };
  let removed: any = { marker: "removed" };
  churn(20_000);
  removedWeak = new WeakRef(removed);
  registry.register(removed, "removed-value", token);
  console.log("unregister:", registry.unregister(token));
  removed = null;
})();

for (let i = 0; i < 6; i++) {
  churn(50_000);
  gc();
  await Promise.resolve();
}

console.log("after deref:", weak.deref() === undefined ? "undefined" : "live");
console.log(
  "after removed:",
  removedWeak.deref() === undefined ? "undefined" : "live",
);
console.log("cleanup:", cleanup.join(",") || "none");
