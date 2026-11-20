/* Callbacks that return the object they built.
 *
 * `openNamed` builds the object in the callback scope. `openScoped` opens its
 * own handle scope, builds the object there, closes the scope and then returns
 * the object: node-addon-api code does this whenever a `Napi::HandleScope`
 * destructor runs before the return value reaches the host. `openNested` does
 * the same through two nested scopes, and `openEscaped` escapes the object
 * from an escapable scope. Node returns the object from all four.
 */
#include <stdint.h>
#include <stddef.h>

#define NAPI_IMPORT __attribute__((visibility("default")))
#define NAPI_EXPORT __attribute__((visibility("default")))

typedef void* napi_env;
typedef void* napi_value;
typedef void* napi_callback_info;
typedef void* napi_handle_scope;
typedef void* napi_escapable_handle_scope;
typedef int32_t napi_status;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);

NAPI_IMPORT napi_status napi_create_function(
    napi_env, const char*, size_t, napi_callback, void*, napi_value*);
NAPI_IMPORT napi_status napi_create_object(napi_env, napi_value*);
NAPI_IMPORT napi_status napi_create_int32(napi_env, int32_t, napi_value*);
NAPI_IMPORT napi_status napi_set_named_property(napi_env, napi_value, const char*, napi_value);
NAPI_IMPORT napi_status napi_open_handle_scope(napi_env, napi_handle_scope*);
NAPI_IMPORT napi_status napi_close_handle_scope(napi_env, napi_handle_scope);
NAPI_IMPORT napi_status napi_open_escapable_handle_scope(napi_env, napi_escapable_handle_scope*);
NAPI_IMPORT napi_status napi_close_escapable_handle_scope(napi_env, napi_escapable_handle_scope);
NAPI_IMPORT napi_status napi_escape_handle(
    napi_env, napi_escapable_handle_scope, napi_value, napi_value*);

static napi_value build(napi_env env, int32_t master) {
  napi_value object = 0;
  napi_value value = 0;
  if (napi_create_object(env, &object) != 0 || napi_create_int32(env, master, &value) != 0 ||
      napi_set_named_property(env, object, "master", value) != 0) {
    return 0;
  }
  return object;
}

static napi_value open_named(napi_env env, napi_callback_info info) {
  (void)info;
  return build(env, 11);
}

static napi_value open_scoped(napi_env env, napi_callback_info info) {
  napi_handle_scope scope = 0;
  napi_value object = 0;
  (void)info;
  if (napi_open_handle_scope(env, &scope) != 0) return 0;
  object = build(env, 31);
  if (napi_close_handle_scope(env, scope) != 0) return 0;
  return object;
}

static napi_value open_nested(napi_env env, napi_callback_info info) {
  napi_handle_scope outer = 0;
  napi_handle_scope inner = 0;
  napi_value object = 0;
  (void)info;
  if (napi_open_handle_scope(env, &outer) != 0 || napi_open_handle_scope(env, &inner) != 0) return 0;
  object = build(env, 41);
  if (napi_close_handle_scope(env, inner) != 0 || napi_close_handle_scope(env, outer) != 0) return 0;
  return object;
}

static napi_value open_escaped(napi_env env, napi_callback_info info) {
  napi_escapable_handle_scope scope = 0;
  napi_value escaped = 0;
  (void)info;
  if (napi_open_escapable_handle_scope(env, &scope) != 0 ||
      napi_escape_handle(env, scope, build(env, 51), &escaped) != 0 ||
      napi_close_escapable_handle_scope(env, scope) != 0) {
    return 0;
  }
  return escaped;
}

NAPI_EXPORT int32_t node_api_module_get_api_version_v1(void) { return 8; }

NAPI_EXPORT napi_value napi_register_module_v1(napi_env env, napi_value exports) {
  static const struct { const char* name; napi_callback callback; } functions[] = {
      {"openNamed", open_named},
      {"openScoped", open_scoped},
      {"openNested", open_nested},
      {"openEscaped", open_escaped},
  };
  for (size_t index = 0; index < sizeof functions / sizeof functions[0]; index++) {
    napi_value function = 0;
    if (napi_create_function(env, functions[index].name, (size_t)-1, functions[index].callback, 0,
                             &function) != 0 ||
        napi_set_named_property(env, exports, functions[index].name, function) != 0) {
      return 0;
    }
  }
  return exports;
}
