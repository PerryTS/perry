// demonstrates: plugin-side perry/plugin API used in docs/src/plugins/{creating-plugins,hooks-and-events}.md
// docs: docs/src/plugins/creating-plugins.md
// platforms: macos, linux, windows
// run: false

// `run: false` because plugin-side `activate(api)` is invoked by the host
// after `dlopen`'ing this file built with `--output-type dylib`. The doc-tests
// harness compiles every example as an executable, so we wrap the plugin body
// in a function `activate` that's never called at top level — the snippet
// still type-checks against `PluginApi` and emits the runtime calls into
// the LLVM IR, so any drift in the `PERRY_PLUGIN_INSTANCE_TABLE` dispatcher
// trips a link error in CI.

import type { PluginApi } from "perry/plugin"

// ANCHOR: counter-plugin
let count = 0

export function activate(api: PluginApi) {
    api.setMetadata("counter", "1.0.0", "Counts hook invocations")

    api.registerHook("onRequest", (data) => {
        count++
        console.log(`Request #${count}`)
        return data
    })

    api.registerTool("getCount", "returns request count", () => count)
}

export function deactivate() {
    console.log(`Total requests processed: ${count}`)
}
// ANCHOR_END: counter-plugin

// ANCHOR: hook-filter
function registerFilter(api: PluginApi) {
    api.registerHook("transform", (data: any) => {
        data.content = data.content.toUpperCase()
        return data // Returned data goes to next plugin
    })
}
// ANCHOR_END: hook-filter

// ANCHOR: hook-action
function registerAction(api: PluginApi) {
    api.registerHook("onSave", (data: any) => {
        console.log(`Saved: ${data.path}`)
        return data
    })
}
// ANCHOR_END: hook-action

// ANCHOR: hook-waterfall
function registerWaterfall(api: PluginApi) {
    api.registerHook("buildMenu", (items: any) => {
        items.push({ label: "My Plugin Action", action: () => {} })
        return items
    })
}
// ANCHOR_END: hook-waterfall

// ANCHOR: hook-priority
function registerPriorities(api: PluginApi, validate: (d: any) => any, transform: (d: any) => any, log: (d: any) => any) {
    // Lower priority numbers run first; default 10. Mode 0=filter / 1=action / 2=waterfall.
    api.registerHookEx("beforeSave", validate, 10, 0)   // Runs first
    api.registerHookEx("beforeSave", transform, 20, 0)  // Runs second
    api.registerHookEx("beforeSave", log, 100, 1)        // Runs last (action mode)
}
// ANCHOR_END: hook-priority

// ANCHOR: emit-from-plugin
function emitFromPlugin(api: PluginApi) {
    api.emit("dataUpdated", { source: "my-plugin", records: 42 })
}
// ANCHOR_END: emit-from-plugin

// ANCHOR: on-event
function listenForEvent(api: PluginApi) {
    api.on("dataUpdated", (data: any) => {
        console.log(`${data.source} updated ${data.records} records`)
    })
}
// ANCHOR_END: on-event

// ANCHOR: register-tool
function registerFormatter(api: PluginApi) {
    api.registerTool("formatCode", "format source code", (args: any) => {
        return `// formatted: ${args.code}`
    })
}
// ANCHOR_END: register-tool

// ANCHOR: read-config
function readConfig(api: PluginApi) {
    const theme = api.getConfig("theme")     // "dark"
    const retries = api.getConfig("maxRetries") // "3"
    return { theme, retries }
}
// ANCHOR_END: read-config

// ANCHOR: register-service
function registerWorker(api: PluginApi) {
    api.registerService(
        "worker",
        () => { console.log("worker started") },
        () => { console.log("worker stopped") },
    )
}
// ANCHOR_END: register-service

// ANCHOR: unregister-hooks
// Selective cleanup for long-lived plugins that re-register at runtime.
// Pass the exact same closure reference that was registered — the runtime
// does a closure-identity compare.
function unregisterHook(api: PluginApi, handler: (data: any) => any) {
    api.unregisterHook("onRequest", handler)
}
// ANCHOR_END: unregister-hooks

// ANCHOR: unregister-tools-services-routes
function unregisterEverything(api: PluginApi) {
    api.unregisterTool("formatCode")
    api.unregisterService("worker")   // stopFn is invoked before removal
    api.unregisterRoute("/api/foo")
}
// ANCHOR_END: unregister-tools-services-routes

// ANCHOR: off-event
function unsubscribe(api: PluginApi, handler: (data: any) => void) {
    api.off("dataUpdated", handler)
}
// ANCHOR_END: off-event

// ANCHOR: deactivate-cleanup
// Recommended pattern: keep a module-scoped reference to handlers so
// `deactivate()` can selectively unregister them. The host also purges
// all of a plugin's registrations on unload, but explicit unregister
// calls are the right way to stop services / event handlers cleanly
// when a plugin re-configures itself at runtime. Shown here as a
// regular function (not exported) so it doesn't collide with the
// `activate` / `deactivate` exports from `counter-plugin` above.
const _onDataUpdated = (data: any) => {
    console.log(`${data.source} updated ${data.records} records`)
}

function _stopWorker() {
    console.log("worker stopped")
}

function _startWorker() {
    console.log("worker started")
}

function cleanupExample(api: PluginApi) {
    api.on("dataUpdated", _onDataUpdated)
    api.registerService("worker", _startWorker, _stopWorker)

    // Later, in your deactivate() export:
    api.off("dataUpdated", _onDataUpdated)
    api.unregisterService("worker")   // invokes _stopWorker before removal
}
// ANCHOR_END: deactivate-cleanup

// Reference every helper so the linker doesn't dead-strip the function bodies.
console.log(typeof activate)
console.log(typeof deactivate)
console.log(typeof registerFilter)
console.log(typeof registerAction)
console.log(typeof registerWaterfall)
console.log(typeof registerPriorities)
console.log(typeof emitFromPlugin)
console.log(typeof listenForEvent)
console.log(typeof registerFormatter)
console.log(typeof readConfig)
console.log(typeof registerWorker)
console.log(typeof unregisterHook)
console.log(typeof unregisterEverything)
console.log(typeof unsubscribe)
console.log(typeof cleanupExample)
