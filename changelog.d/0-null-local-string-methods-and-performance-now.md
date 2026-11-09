A local declared `null` or `undefined` and later assigned a string
(`let s = null; s = "<"; s.includes("<")`) dispatches `includes`, `indexOf`,
`lastIndexOf` and `slice` as string methods again; they used to run the array
fast path and answer `false` / `-1` (three.js's `LoopNode` emitted `i--`
for `i++`). `performance.now()` is no longer compiled to a direct clock read,
so a replacement (`Object.defineProperty(performance, "now", …)`,
`performance.now = f`) is called like Node does.
