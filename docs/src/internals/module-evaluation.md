# Module evaluation

Programs without top-level await keep the synchronous module initializer path.
When the compiled graph contains top-level await, each module owns an evaluation
record beside its globals. Static dependencies are visited in source request
order, including cycle edges, using InnerModuleEvaluation. ExecuteAsyncModule
starts a resumable body without draining promise jobs while visiting siblings.
Dependent bodies wait for async dependencies; GatherAvailableAncestors executes
ready ancestors in their async evaluation order. A rejected body rejects its
parents without executing their bodies.

Dynamic import queues evaluation and resolves its namespace only after the
target cycle root completes. Repeated imports use the same module record and
namespace, including while evaluation is pending. Synchronous CommonJS require
of a graph containing top-level await throws ERR_REQUIRE_ASYNC_MODULE.

The runtime fields correspond to the ES module record status, DFS indices,
cycle root, async evaluation order, pending async dependencies and async parent
modules. PendingAsyncDependencies is the current specification name of the
pending async subgraph count. The record's capability, error and body promise
are mutable GC roots. Dependency visitors carry direct module record addresses;
there is no module lookup registry in this evaluator.

See [ECMAScript module evaluation](https://tc39.es/ecma262/multipage/ecmascript-language-scripts-and-modules.html#sec-innermoduleevaluation).
