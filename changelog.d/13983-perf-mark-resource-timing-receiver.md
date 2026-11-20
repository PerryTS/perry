`performance.markResourceTiming` no longer brand-checks its receiver, matching
Node (which brand-checks every other `Performance` method but not
`markResourceTiming`, `eventLoopUtilization` or `timerify`). A detached call
such as undici's module-scope `const markResourceTiming =
performance.markResourceTiming` used to throw `TypeError [ERR_INVALID_THIS]`,
so every `undici.fetch()` compiled from source rejected with `TypeError: fetch
failed` and network errors escaped uncaught from `finalizeAndReportTiming`
(#13964).
