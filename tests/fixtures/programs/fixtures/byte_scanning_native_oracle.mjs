// Node oracle for Perry's explicit NativeArena disposal contract. Perry uses
// its real arena implementation; Node checks the same indexed-read contract.
import { registerHooks } from "node:module";
registerHooks({ resolve(specifier, context, next) {
  return specifier === "perry/native" ? { url: import.meta.url, shortCircuit: true } : next(specifier, context);
} });
export const NativeArena = { alloc(size) {
  const backing = new ArrayBuffer(size);
  let disposed = false;
  return {
    view(kind, offset, length) {
      const bytes = new kind(backing, offset, length);
      return new Proxy(bytes, {
        get(target, key) {
          if (disposed && typeof key === "string" && /^\d+$/.test(key)) throw new TypeError("NativeArena has been disposed");
          return Reflect.get(target, key, target);
        },
        set(target, key, value) { return Reflect.set(target, key, value, target); },
      });
    },
    dispose() { disposed = true; },
  };
} };
