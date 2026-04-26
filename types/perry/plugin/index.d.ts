// Type declarations for perry/plugin — Perry's native plugin system.
// Plugins are compiled to shared libraries (.dylib / .so) loaded at runtime
// via dlopen. One GC, one arena, one runtime — plugin symbols bind to the
// host executable's copies at load time.

// ---------------------------------------------------------------------------
// PluginId — opaque handle returned by loadPlugin
// ---------------------------------------------------------------------------

/** Opaque handle to a loaded plugin. Pass to unloadPlugin or use as a PluginApi. */
export type PluginId = number & { readonly __pluginId: unique symbol };

// ---------------------------------------------------------------------------
// PluginApi — instance methods available on a loaded plugin handle
// ---------------------------------------------------------------------------

/**
 * API handle passed to a plugin's `plugin_activate` function and returned
 * by `loadPlugin`. Call instance methods on this handle to register hooks,
 * tools, services, and routes on behalf of the plugin.
 */
export interface PluginApi {
    /**
     * Register a hook handler (priority=10, mode=filter by default).
     * @param hookName  Name of the hook to listen for.
     * @param handler   Closure receiving the hook context; its return value
     *                  becomes the new context (filter mode).
     */
    registerHook(hookName: string, handler: (ctx: unknown) => unknown): void;

    /**
     * Register a hook handler with explicit priority and execution mode.
     * @param hookName  Name of the hook.
     * @param handler   Hook handler closure.
     * @param priority  Lower numbers run first (default 10).
     * @param mode      0=filter (chain), 1=action (fire-and-forget), 2=waterfall (first result wins).
     */
    registerHookEx(hookName: string, handler: (ctx: unknown) => unknown, priority: number, mode: number): void;

    /**
     * Register a named tool with a description and handler.
     * @param name        Tool name (must be unique across all plugins).
     * @param description Human-readable description of the tool.
     * @param handler     Closure called when the tool is invoked via `invokeTool`.
     */
    registerTool(name: string, description: string, handler: (args: unknown) => unknown): void;

    /**
     * Register a service with start and stop lifecycle functions.
     * @param name     Service name.
     * @param startFn  Called when the service is started.
     * @param stopFn   Called when the service is stopped.
     */
    registerService(name: string, startFn: () => void, stopFn: () => void): void;

    /**
     * Register an HTTP route handler (requires an HTTP plugin integration).
     * @param path     Route path (e.g. "/api/foo").
     * @param handler  Request handler closure.
     */
    registerRoute(path: string, handler: (req: unknown) => unknown): void;

    /**
     * Get a host-provided config value by key.
     * @param key  Config key set via `setPluginConfig`.
     * @returns    The stored value, or `undefined` if not set.
     */
    getConfig(key: string): unknown;

    /**
     * Log a message at the given level.
     * @param level    0=DEBUG, 1=INFO, 2=WARN, 3=ERROR.
     * @param message  Message to log (written to stderr with plugin prefix).
     */
    log(level: number, message: string): void;

    /**
     * Set plugin metadata (name, version, description).
     * Must be called during `plugin_activate`; has no effect after load.
     */
    setMetadata(name: string, version: string, description: string): void;

    /**
     * Subscribe to an event on the host event bus.
     * @param event    Event name.
     * @param handler  Closure called when the event is emitted.
     */
    on(event: string, handler: (data: unknown) => void): void;

    /**
     * Emit an event on the host event bus, dispatching to all subscribers.
     * @param event  Event name.
     * @param data   Payload forwarded to every subscriber.
     */
    emit(event: string, data: unknown): void;
}

// ---------------------------------------------------------------------------
// Host-side API (static, receiver-less)
// ---------------------------------------------------------------------------

/**
 * Load a plugin from a shared library path (.dylib / .so).
 * Calls the plugin's `plugin_activate(api_handle)` entry point.
 * @param path  Absolute or relative path to the shared library.
 * @returns     A PluginId handle (> 0) on success, or 0 on failure.
 */
export function loadPlugin(path: string): PluginId;

/**
 * Unload a previously loaded plugin by its ID.
 * Calls `plugin_deactivate()` if the symbol is present, then dlcloses the library.
 * All hooks, tools, services, and routes registered by the plugin are removed.
 */
export function unloadPlugin(id: PluginId): void;

/**
 * Emit a hook, calling all registered handlers in priority order.
 * The hook mode (filter/action/waterfall) is determined per handler at registration.
 * @param hookName  Name of the hook.
 * @param context   Initial context value threaded through handlers (filter mode).
 * @returns         The final context value after all handlers have run.
 */
export function emitHook(hookName: string, context: unknown): unknown;

/**
 * Emit an event on the host event bus, dispatching to all subscribers.
 * @param event  Event name.
 * @param data   Payload forwarded to every subscriber.
 */
export function emitEvent(event: string, data: unknown): void;

/**
 * Invoke a registered tool by name.
 * @param name  Tool name as registered via `registerTool`.
 * @param args  Arguments forwarded to the tool handler.
 * @returns     The tool handler's return value, or `undefined` if not found.
 */
export function invokeTool(name: string, args: unknown): unknown;

/**
 * Set a host-provided config value (readable by plugins via `api.getConfig`).
 * Can be called before or after loading plugins.
 */
export function setPluginConfig(key: string, value: unknown): void;

/**
 * Scan a directory for plugin files (.dylib / .so / .dll).
 * @param dir  Directory path to scan.
 * @returns    Array of absolute file paths to discovered plugin libraries.
 */
export function discoverPlugins(dir: string): string[];

/**
 * List all currently loaded plugins.
 * @returns Array of `{ id, name, version, description }` objects.
 */
export function listPlugins(): Array<{ id: number; name: string; version: string; description: string }>;

/**
 * List all registered hook names.
 * @returns Array of hook name strings.
 */
export function listHooks(): string[];

/**
 * List all registered tools.
 * @returns Array of `{ name, description, pluginId }` objects.
 */
export function listTools(): Array<{ name: string; description: string; pluginId: number }>;

/**
 * Returns the number of currently loaded plugins.
 */
export function pluginCount(): number;

/**
 * Initialize the plugin registry (call once before loading any plugins).
 * Safe to call multiple times; subsequent calls are no-ops.
 */
export function initPlugins(): void;
