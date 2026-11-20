const os = process.getBuiltinModule("node:os");

function probe(label: string, target: any, key: string, namedRead: () => any) {
  const original = target[key];
  const prior = Object.getOwnPropertyDescriptor(target, key);
  let reads = 0;
  let receiver = true;
  function getter() { reads++; receiver = receiver && this === target; return 37; }
  function setter(_value: any) {}
  Object.defineProperty(target, key, { get: getter, set: setter, enumerable: false, configurable: true });
  const descriptor = Object.getOwnPropertyDescriptor(target, key)!;
  console.log(label, "descriptor", reads, descriptor.get === getter, descriptor.set === setter,
    descriptor.enumerable === false, descriptor.configurable === true, !("value" in descriptor));
  console.log(label, "values", namedRead() === 37, Reflect.get(target, key) === 37, target[key] === 37);
  console.log(label, "reads", reads, receiver);
  const alternate = {};
  Object.defineProperty(target, key, { get() { return this; }, configurable: true });
  console.log(label, "explicit receiver", Reflect.get(target, key, alternate) === alternate);
  const callable = function () { return "callable"; };
  Object.defineProperty(target, key, { get: () => callable, configurable: true });
  const value = namedRead();
  console.log(label, "callable", value === callable, typeof value === "function" ? value() : "not-callable");
  Object.defineProperty(target, key, { get: undefined, set: setter, configurable: true });
  console.log(label, "getterless", namedRead() === undefined, Reflect.get(target, key) === undefined);
  const marker = {};
  Object.defineProperty(target, key, { get() { throw marker; }, configurable: true });
  const outcomes: boolean[] = [];
  for (const read of [namedRead, () => Reflect.get(target, key), () => target[key]]) {
    try { read(); outcomes.push(false); } catch (error) { outcomes.push(error === marker); }
  }
  console.log(label, "throws", outcomes.join(","));
  Object.defineProperty(target, key, { value: 17, writable: true, configurable: true });
  target[key] = 23;
  console.log(label, "data", namedRead() === 23, Reflect.get(target, key) === 23);
  Object.defineProperty(target, key, prior ?? { value: original, writable: true, configurable: true });
  target[key] = original;
}

probe("process", process, "emit", () => process.emit);
probe("os", os, "hostname", () => os.hostname);
const plain: any = {};
probe("plain", plain, "value", () => plain.value);
