#include <stdint.h>
#include <stddef.h>

/* An open addon: it reads the global object and its properties and calls
 * what it finds, so it reaches `Function` and `process.getBuiltinModule`
 * without the program handing it either. */

typedef void* napi_env;
typedef void* napi_value;
typedef void* napi_callback_info;
typedef int32_t napi_status;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);

#define NAPI_IMPORT __attribute__((visibility("default")))
#define NAPI_EXPORT __attribute__((visibility("default")))

NAPI_IMPORT napi_status napi_get_global(napi_env, napi_value*);
NAPI_IMPORT napi_status napi_get_named_property(napi_env, napi_value, const char*, napi_value*);
NAPI_IMPORT napi_status napi_call_function(
    napi_env, napi_value, napi_value, size_t, const napi_value*, napi_value*);
NAPI_IMPORT napi_status napi_create_function(
    napi_env, const char*, size_t, napi_callback, void*, napi_value*);
NAPI_IMPORT napi_status napi_get_cb_info(
    napi_env, napi_callback_info, size_t*, napi_value*, napi_value*, void**);
NAPI_IMPORT napi_status napi_set_named_property(
    napi_env, napi_value, const char*, napi_value);

/* run(source): new Function(source)() through the global `Function`. */
static napi_value run(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value source = 0;
  napi_value global = 0;
  napi_value function_ctor = 0;
  napi_value body = 0;
  napi_value result = 0;
  if (napi_get_cb_info(env, info, &argc, &source, 0, 0) != 0 || argc != 1 ||
      napi_get_global(env, &global) != 0 ||
      napi_get_named_property(env, global, "Function", &function_ctor) != 0 ||
      napi_call_function(env, global, function_ctor, 1, &source, &body) != 0 ||
      napi_call_function(env, global, body, 0, 0, &result) != 0) {
    return 0;
  }
  return result;
}

/* builtin(id): globalThis.process.getBuiltinModule(id). */
static napi_value builtin(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value id = 0;
  napi_value global = 0;
  napi_value process = 0;
  napi_value get_builtin_module = 0;
  napi_value result = 0;
  if (napi_get_cb_info(env, info, &argc, &id, 0, 0) != 0 || argc != 1 ||
      napi_get_global(env, &global) != 0 ||
      napi_get_named_property(env, global, "process", &process) != 0 ||
      napi_get_named_property(env, process, "getBuiltinModule", &get_builtin_module) != 0 ||
      napi_call_function(env, process, get_builtin_module, 1, &id, &result) != 0) {
    return 0;
  }
  return result;
}

NAPI_EXPORT int32_t node_api_module_get_api_version_v1(void) { return 8; }

NAPI_EXPORT napi_value napi_register_module_v1(napi_env env, napi_value exports) {
  napi_value run_function = 0;
  napi_value builtin_function = 0;
  if (napi_create_function(env, "run", 3, run, 0, &run_function) != 0 ||
      napi_set_named_property(env, exports, "run", run_function) != 0 ||
      napi_create_function(env, "builtin", 7, builtin, 0, &builtin_function) != 0 ||
      napi_set_named_property(env, exports, "builtin", builtin_function) != 0) {
    return 0;
  }
  return exports;
}
