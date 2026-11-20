/* Environment teardown work that reports when it runs.
 *
 * `hold(referenced)` registers, in this order, an environment cleanup hook
 * ("cleanup-first"), one thread-safe function with no JS callback and no
 * worker thread (its finalizer writes "finalized"), an async cleanup hook
 * without a removal handle ("async-cleanup") and a second cleanup hook
 * ("cleanup"). With `referenced` false the function is unref'd, so it does
 * not keep the event loop alive.
 *
 * Node runs all of them when the environment is torn down (the natural end
 * of the event loop): the cleanup queue newest first (the thread-safe
 * function's own entry only closes it), then the finalizer of the closed
 * function. `process.exit()` does not tear the environment down, so there
 * none runs.
 */
#include <stdint.h>
#include <stddef.h>
#include <unistd.h>

#define NAPI_IMPORT __attribute__((visibility("default")))
#define NAPI_EXPORT __attribute__((visibility("default")))

typedef void* napi_env;
typedef void* napi_value;
typedef void* napi_callback_info;
typedef void* napi_threadsafe_function;
typedef int32_t napi_status;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);
typedef void (*napi_finalize)(napi_env, void*, void*);
typedef void (*napi_threadsafe_function_call_js)(napi_env, napi_value, void*, void*);

NAPI_IMPORT napi_status napi_create_function(
    napi_env, const char*, size_t, napi_callback, void*, napi_value*);
NAPI_IMPORT napi_status napi_get_cb_info(
    napi_env, napi_callback_info, size_t*, napi_value*, napi_value*, void**);
NAPI_IMPORT napi_status napi_get_value_bool(napi_env, napi_value, _Bool*);
NAPI_IMPORT napi_status napi_get_undefined(napi_env, napi_value*);
NAPI_IMPORT napi_status napi_create_string_utf8(napi_env, const char*, size_t, napi_value*);
NAPI_IMPORT napi_status napi_set_named_property(napi_env, napi_value, const char*, napi_value);
NAPI_IMPORT napi_status napi_create_threadsafe_function(
    napi_env, napi_value, napi_value, napi_value, size_t, size_t, void*, napi_finalize, void*,
    napi_threadsafe_function_call_js, napi_threadsafe_function*);
NAPI_IMPORT napi_status napi_unref_threadsafe_function(napi_env, napi_threadsafe_function);
NAPI_IMPORT napi_status napi_add_env_cleanup_hook(napi_env, void (*)(void*), void*);
typedef void* napi_async_cleanup_hook_handle;
NAPI_IMPORT napi_status napi_add_async_cleanup_hook(
    napi_env, void (*)(napi_async_cleanup_hook_handle, void*), void*,
    napi_async_cleanup_hook_handle*);
NAPI_IMPORT napi_status napi_remove_async_cleanup_hook(napi_async_cleanup_hook_handle);

static void call_js(napi_env env, napi_value js_callback, void* context, void* data) {
  (void)env;
  (void)js_callback;
  (void)context;
  (void)data;
}

static void finalize(napi_env env, void* data, void* hint) {
  static const char line[] = "finalized\n";
  (void)env;
  (void)data;
  (void)hint;
  (void)!write(1, line, sizeof line - 1);
}

static void say(const char* line) {
  size_t length = 0;
  while (line[length]) length++;
  (void)!write(1, line, length);
}

static void cleanup(void* argument) {
  say(argument ? "cleanup-first\n" : "cleanup\n");
}

static void async_cleanup(napi_async_cleanup_hook_handle handle, void* argument) {
  (void)argument;
  say("async-cleanup\n");
  (void)napi_remove_async_cleanup_hook(handle);
}

static napi_value hold(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {0};
  _Bool referenced = 1;
  napi_value name = 0;
  napi_value result = 0;
  napi_threadsafe_function tsfn = 0;
  if (napi_get_cb_info(env, info, &argc, argv, 0, 0) != 0 || argc != 1 ||
      napi_get_value_bool(env, argv[0], &referenced) != 0 ||
      napi_create_string_utf8(env, "fixture-exit-finalizer", (size_t)-1, &name) != 0 ||
      napi_add_env_cleanup_hook(env, cleanup, (void*)1) != 0 ||
      napi_create_threadsafe_function(env, 0, 0, name, 0, 1, 0, finalize, 0, call_js, &tsfn) != 0 ||
      (!referenced && napi_unref_threadsafe_function(env, tsfn) != 0) ||
      napi_add_async_cleanup_hook(env, async_cleanup, 0, 0) != 0 ||
      napi_add_env_cleanup_hook(env, cleanup, 0) != 0 ||
      napi_get_undefined(env, &result) != 0) {
    return 0;
  }
  return result;
}

NAPI_EXPORT int32_t node_api_module_get_api_version_v1(void) { return 8; }

NAPI_EXPORT napi_value napi_register_module_v1(napi_env env, napi_value exports) {
  napi_value hold_function = 0;
  if (napi_create_function(env, "hold", 4, hold, 0, &hold_function) != 0 ||
      napi_set_named_property(env, exports, "hold", hold_function) != 0) {
    return 0;
  }
  return exports;
}
