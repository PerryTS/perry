On the web/wasm target, `fetch(url)` and other calls of global functions from WASM code now run with `globalThis` as the receiver instead of silently returning `undefined`, so the request is actually sent.
Host calls and callbacks preserve exception values and run the correct catch/finally cleanup across function calls and loop exits.
