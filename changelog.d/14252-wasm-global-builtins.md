On the web/wasm target, a builtin read as a value from ordinary code (`const P = Promise`, the `Promise` in `Promise.resolve(x)`) now resolves on `globalThis` instead of silently reading `undefined`.
