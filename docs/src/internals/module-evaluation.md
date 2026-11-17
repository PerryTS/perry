# Module evaluation

Programs without top-level await keep the synchronous module initializer path.
When the compiled graph contains top-level await, each module owns an evaluation
record beside its globals. Static dependencies are visited in source request
order, including cycle edges, using InnerModuleEvaluation. ExecuteAsyncModule
starts a resumable body without draining promise jobs while visiting siblings.
Dependent bodies wait for async dependencies; GatherAvailableAncestors executes
ready ancestors in their async evaluation order. A rejected body rejects its
parents without executing their bodies. A native completion callback finishes
module-owned class statics after the resumable body fulfils, before the record
completes or ancestors become available. A completion throw rejects the same
evaluation capability with the original error value.

Dynamic import queues evaluation and resolves its namespace only after the
target cycle root completes. Repeated imports use the same module record and
namespace, including while evaluation is pending. Synchronous CommonJS require
of a graph containing top-level await throws ERR_REQUIRE_ASYNC_MODULE.

The runtime tracks [[AsyncEvaluation]], [[PendingAsyncSubgraphsCount]] and
[[AsyncParentModules]], together with module status, DFS indices, cycle roots
and async evaluation order. The record's capability, error and body promise
are mutable GC roots. Dependency visitors carry direct module record addresses;
there is no module lookup registry in this evaluator.

See [ECMAScript module evaluation](https://tc39.es/ecma262/multipage/ecmascript-language-scripts-and-modules.html#sec-innermoduleevaluation).

The HIR module names its resumable body with `async_init_function: FuncId`.
Activation follows that explicit execution edge when the owning module runs,
so dynamic imports in the body remain reachable without exporting the body or
recognizing its generated name. Modules without top-level await carry no edge.

## Native module value exports

Dynamic constant and namespace reads use per-module getter slots in
`object/native_module/constant_getters.rs`. The existing `js_nm_install_*`
entry points register these alongside method dispatch. An unknown dynamic
module specifier uses the compiler's install-all path, including the legacy
`node:constants` aggregate. The dynamic-code installer retains every constant
getter because interpreted code can obtain modules by runtime name. Materialized
process, module-loader and global objects, and open native addons, also retain
all getters: a computed property can obtain a loader without naming it in the
source. Dedicated process intrinsics such as `process.argv` expose no loader. Computed
property names do not narrow selection:
the installed getter covers its module's complete constant surface.
Alias normalization runs before getter dispatch. Each getter passes a literal
normalized module name to the shared resolver, so an opaque alias lookup cannot
keep unrelated module arms reachable through that getter.

Statically selected value exports register their module's getter before
reading the export cache, so a returned constants object remains readable through
dynamic paths even when no whole-module installer runs. The `fs/promises`
and global WebCrypto installers and namespace factories register their own
getters, including when a namespace is already cached. WebCrypto has a separate slot from Node crypto's constant table.
Slots contain only function pointers and use process-wide atomic publication;
workers share the installed code, while namespace values retain their existing
per-thread caches and GC roots. The same runtime getters serve native and WASI
builds; the constants-only installer is included in the WASI ABI inventory.
