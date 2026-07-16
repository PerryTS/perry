import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
for (
  const [label, object] of [["require", req], ["resolve", req.resolve]] as const
) {
  console.log(
    label,
    object.name,
    object.length,
    Object.getOwnPropertyNames(object).sort().join(","),
  );
  for (const key of Object.getOwnPropertyNames(object).sort()) {
    if (["arguments", "caller", "prototype"].includes(key)) continue;
    const descriptor = Object.getOwnPropertyDescriptor(object, key)!;
    console.log(
      label,
      key,
      JSON.stringify({
        enumerable: descriptor.enumerable,
        writable: descriptor.writable,
        configurable: descriptor.configurable,
        type: typeof descriptor.value,
      }),
    );
  }
}
console.log(
  "shared globals:",
  req.cache === createRequire(new URL("./other.cjs", import.meta.url)).cache,
  req.extensions === createRequire(import.meta.url).extensions,
);
