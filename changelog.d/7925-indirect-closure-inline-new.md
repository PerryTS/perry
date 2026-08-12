### Indirect pipeline closures inline their bounded hot allocations

The allocation-hot pre-pass previously recognized only direct `FuncRef` calls.
A closure reached through an array or local value therefore kept calling the
outlined object allocator even when the call itself sat in a hot loop. The
pipeline workload paid that call for each of its three stages on every record.

Modules with an indirect loop call now admit allocation-bearing closure bodies
when the module contains at most eight closure `new` sites in total. The cap is
all-or-none and counts nested expression containers and parameter defaults, so
speculative code growth is bounded and independent of traversal order. Closure
compilation now propagates the admission bit to the allocator gate.

On the 19-program corpus, 18 binaries remain byte-identical and only `pipeline`
changes. Its text grows by 276 bytes (+0.0030%; no executable-size change) while
retired instructions fall by 13.89% on the quiet M1 benchmark host. All corpus
outputs remain exact.
