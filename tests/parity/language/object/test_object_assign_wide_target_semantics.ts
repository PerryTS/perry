// Object.assign onto targets with no inline room left must keep strict
// [[Set]]: inherited setters, read-only properties, frozen and non-extensible
// targets, and array index keys.
function wide(target: any, n: number, prefix = "k"): any {
  for (let i = 0; i < n; i++) target[prefix + i] = i;
  return target;
}
function attempt(label: string, run: () => void): void {
  try {
    run();
    console.log(label, "ok");
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}
function source(n: number): any {
  const s: any = {};
  for (let i = 0; i < n; i++) s["s" + i] = i * 10;
  return s;
}

const seen: string[] = [];
class WithSetter {
  set s20(value: number) {
    seen.push("s20=" + value);
  }
}
const instance = wide(new WithSetter(), 64);
Object.assign(instance, { s20: 1, s21: 2 });
console.log("setter", seen.join(","), Object.hasOwn(instance, "s20"), instance.s21, Object.keys(instance).length);

const proto = {};
Object.defineProperty(proto, "ro", { value: 1, writable: false });
const inheritsReadOnly = wide(Object.create(proto), 64);
attempt("inherited read-only", () => Object.assign(inheritsReadOnly, { ro: 2 }));
console.log("inherited read-only", Object.hasOwn(inheritsReadOnly, "ro"), inheritsReadOnly.ro);

const frozen = Object.freeze(wide({}, 64));
attempt("frozen existing", () => Object.assign(frozen, { k3: 99 }));
attempt("frozen new", () => Object.assign(frozen, { fresh: 1 }));
console.log("frozen", frozen.k3, Object.hasOwn(frozen, "fresh"));

const sealed = Object.preventExtensions(wide({}, 64));
attempt("non-extensible existing", () => Object.assign(sealed, { k5: 55 }));
attempt("non-extensible new", () => Object.assign(sealed, { fresh: 1 }));
console.log("non-extensible", sealed.k5, Object.hasOwn(sealed, "fresh"));

const ordered = wide({}, 64);
Object.assign(ordered, { k0: "a", extra: "b", k63: "c" });
const keys = Object.keys(ordered);
console.log("order", keys[0], keys[63], keys[64], ordered.k0, ordered.extra, ordered.k63, keys.length);

const arr: any = [1, 2, 3];
wide(arr, 40, "p");
Object.assign(arr, { 0: "a", 5: "f", p7: "seven" }, ["x"]);
console.log("array", JSON.stringify(arr), arr.length, arr.p7, arr.p39);
