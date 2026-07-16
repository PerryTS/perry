import { SourceMap } from "node:module";

console.log(
  "shape:",
  SourceMap.name,
  SourceMap.length,
  Object.getOwnPropertyNames(SourceMap.prototype).sort().join(","),
);
for (const key of Object.getOwnPropertyNames(SourceMap.prototype).sort()) {
  const descriptor = Object.getOwnPropertyDescriptor(SourceMap.prototype, key)!;
  console.log(
    key,
    JSON.stringify({
      enumerable: descriptor.enumerable,
      configurable: descriptor.configurable,
      writable: descriptor.writable,
      get: typeof descriptor.get,
      value: typeof descriptor.value,
    }),
  );
}

for (const value of [undefined, null, 1, true, "map", []]) {
  try {
    new SourceMap(value as any);
    console.log(typeof value, "no throw");
  } catch (error) {
    console.log(
      typeof value,
      (error as any).name,
      (error as any).code ?? "no-code",
    );
  }
}
