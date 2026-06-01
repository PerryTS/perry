// parity-node-argv: --experimental-webstorage --localstorage-file=/tmp/perry_web_storage_parity.sqlite

const g: any = globalThis;

function show(label: string, value: unknown) {
  console.log(label + ":", String(value));
}

function showError(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ":", "no throw");
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.message);
  }
}

function globalDesc(label: string, name: string) {
  const desc: any = Object.getOwnPropertyDescriptor(globalThis, name);
  console.log(
    label + ":",
    !!desc,
    typeof desc?.value,
    typeof desc?.get,
    typeof desc?.set,
    desc?.writable,
    desc?.enumerable,
    desc?.configurable,
  );
}

localStorage.clear();
sessionStorage.clear();

show("typeof Storage", typeof Storage);
show("typeof localStorage", typeof localStorage);
show("typeof sessionStorage", typeof sessionStorage);
show("Storage name length", Storage.name + "/" + Storage.length);
show("same storage globals", localStorage === sessionStorage);

globalDesc("Storage desc", "Storage");
globalDesc("localStorage desc", "localStorage");
globalDesc("sessionStorage desc", "sessionStorage");

for (const name of ["clear", "getItem", "key", "removeItem", "setItem"]) {
  const fn = (Storage.prototype as any)[name];
  const desc: any = Object.getOwnPropertyDescriptor(Storage.prototype, name);
  console.log(
    "proto " + name + ":",
    typeof fn,
    fn?.name,
    fn?.length,
    desc?.writable,
    desc?.enumerable,
    desc?.configurable,
  );
}

sessionStorage.setItem("b", 2 as any);
sessionStorage.setItem("a", null as any);
sessionStorage.setItem(3 as any, undefined as any);
show("session length", sessionStorage.length);
show("session keys", [0, 1, 2, 3].map((i) => sessionStorage.key(i)).join("|"));
show(
  "session values",
  [
    sessionStorage.getItem("b"),
    sessionStorage.getItem("a"),
    sessionStorage.getItem("3"),
    sessionStorage.getItem("missing"),
  ].join("|"),
);

sessionStorage.setItem("b", "updated");
show("session keys after update", [0, 1, 2].map((i) => sessionStorage.key(i)).join("|"));
show("session b after update", sessionStorage.getItem("b"));
show("named a", (sessionStorage as any).a);

sessionStorage.setItem("getItem", "shadow");
show("method collision", typeof sessionStorage.getItem + "/" + sessionStorage.getItem("getItem"));

localStorage.setItem("a", "local");
show("isolation", localStorage.getItem("a") + "/" + sessionStorage.getItem("a"));

sessionStorage.removeItem("a");
show("after remove", sessionStorage.length + "/" + sessionStorage.getItem("a") + "/" + (sessionStorage as any).a);
sessionStorage.clear();
show("after clear", sessionStorage.length + "/" + sessionStorage.key(0));

const rebound = sessionStorage.getItem;
showError("rebound getItem", () => rebound("x"));
showError("Storage call", () => g.Storage());
showError("Storage construct", () => new g.Storage());
