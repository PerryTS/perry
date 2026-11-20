The element-shape versioned loop clone (#7480/#7669/#7701) now admits the body
form real read loops are written in: `for (let i = 0; i < a.length; i++)
{ const r = a[i]; s += r.x + r.y; }` (#7771). The binding is virtual inside
the fast clone — its `Let` emits nothing, every `r.field` read lowers through
the loop fact to a bare element load under the preheader guard, and the
binding's lexical-death shadow-slot clear is suppressed there (in the
call-fallback shadow mode that clear is a runtime call, which would fail the
clone's call-free admission scan and silently delete it, #7690). The read
loop's fast clone now contains no calls at all: the bounded-index guard
diamond, its `js_array_get_f64` slow arm, both `js_number_coerce` diamonds and
the back-edge poll are gone from the hot path (pinned mini, interleaved
best-of-5 over 50M fetches: 0.07–0.08s → 0.05s user). `const`-only by design
(`var` is observable after the loop); the slow clone still binds generically
and every guard-declined shape (holes, `undefined` elements, Array subclasses,
proxies, shrunk length, mid-loop mutation) is byte-verified against node in
`test-files/test_gap_7771_*.ts`. Found and filed while validating: #7775
(module-wide read-loop miscompile triggered by an uncalled `new Proxy(arr,{})`)
and #7776 (`NaN` where node string-concatenates after an `as any`
heterogeneous element store) — both pre-existing on pristine main.
