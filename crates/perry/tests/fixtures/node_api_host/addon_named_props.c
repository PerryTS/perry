/* Node-API named property operations on objects JavaScript built and on
 * objects the addon builds.
 *
 * `setNamed`, `getNamed` and `hasNamed` forward one name to
 * napi_set_named_property, napi_get_named_property and
 * napi_has_named_property; `setNamedStatus` reports the status a named store
 * returns (a pending exception is cleared and reported as its message).
 * `make(n)` builds n objects with five named stores each, the way an addon
 * reports a packet. Node's answers are the expected transcript.
 */
#include <stdint.h>
#include <stddef.h>

#define NAPI_IMPORT __attribute__((visibility("default")))
#define NAPI_EXPORT __attribute__((visibility("default")))

typedef void* napi_env;
typedef void* napi_value;
typedef void* napi_callback_info;
typedef int32_t napi_status;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);

NAPI_IMPORT napi_status napi_create_function(
    napi_env, const char*, size_t, napi_callback, void*, napi_value*);
NAPI_IMPORT napi_status napi_get_cb_info(
    napi_env, napi_callback_info, size_t*, napi_value*, napi_value*, void**);
NAPI_IMPORT napi_status napi_get_value_string_utf8(napi_env, napi_value, char*, size_t, size_t*);
NAPI_IMPORT napi_status napi_get_value_uint32(napi_env, napi_value, uint32_t*);
NAPI_IMPORT napi_status napi_create_object(napi_env, napi_value*);
NAPI_IMPORT napi_status napi_create_array_with_length(napi_env, size_t, napi_value*);
NAPI_IMPORT napi_status napi_create_int32(napi_env, int32_t, napi_value*);
NAPI_IMPORT napi_status napi_create_string_utf8(napi_env, const char*, size_t, napi_value*);
NAPI_IMPORT napi_status napi_get_boolean(napi_env, int, napi_value*);
NAPI_IMPORT napi_status napi_get_undefined(napi_env, napi_value*);
NAPI_IMPORT napi_status napi_set_element(napi_env, napi_value, uint32_t, napi_value);
NAPI_IMPORT napi_status napi_set_named_property(napi_env, napi_value, const char*, napi_value);
NAPI_IMPORT napi_status napi_get_named_property(napi_env, napi_value, const char*, napi_value*);
NAPI_IMPORT napi_status napi_has_named_property(napi_env, napi_value, const char*, _Bool*);
NAPI_IMPORT napi_status napi_is_exception_pending(napi_env, _Bool*);
NAPI_IMPORT napi_status napi_get_and_clear_last_exception(napi_env, napi_value*);

/* args[0] = object, args[1] = name, args[2] = value */
static int args(napi_env env, napi_callback_info info, napi_value* argv, char* name, size_t cap) {
  size_t argc = 3;
  size_t len = 0;
  if (napi_get_cb_info(env, info, &argc, argv, 0, 0) != 0) return 0;
  return napi_get_value_string_utf8(env, argv[1], name, cap, &len) == 0;
}

static napi_value set_named(napi_env env, napi_callback_info info) {
  napi_value argv[3] = {0, 0, 0};
  char name[64];
  napi_value undefined = 0;
  if (!args(env, info, argv, name, sizeof name)) return 0;
  napi_set_named_property(env, argv[0], name, argv[2]);
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value set_named_status(napi_env env, napi_callback_info info) {
  napi_value argv[3] = {0, 0, 0};
  char name[64];
  napi_value result = 0;
  _Bool pending = 0;
  if (!args(env, info, argv, name, sizeof name)) return 0;
  napi_status status = napi_set_named_property(env, argv[0], name, argv[2]);
  napi_is_exception_pending(env, &pending);
  if (pending) {
    napi_value exception = 0;
    napi_get_and_clear_last_exception(env, &exception);
    napi_create_string_utf8(env, "threw", (size_t)-1, &result);
    return result;
  }
  napi_create_int32(env, status, &result);
  return result;
}

static napi_value get_named(napi_env env, napi_callback_info info) {
  napi_value argv[3] = {0, 0, 0};
  char name[64];
  napi_value result = 0;
  if (!args(env, info, argv, name, sizeof name)) return 0;
  if (napi_get_named_property(env, argv[0], name, &result) != 0) return 0;
  return result;
}

static napi_value has_named(napi_env env, napi_callback_info info) {
  napi_value argv[3] = {0, 0, 0};
  char name[64];
  napi_value result = 0;
  _Bool present = 0;
  if (!args(env, info, argv, name, sizeof name)) return 0;
  if (napi_has_named_property(env, argv[0], name, &present) != 0) return 0;
  napi_get_boolean(env, present, &result);
  return result;
}

static napi_value make(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {0};
  uint32_t count = 0;
  napi_value list = 0;
  if (napi_get_cb_info(env, info, &argc, argv, 0, 0) != 0 ||
      napi_get_value_uint32(env, argv[0], &count) != 0 ||
      napi_create_array_with_length(env, count, &list) != 0) {
    return 0;
  }
  for (uint32_t index = 0; index < count; index++) {
    napi_value object = 0, msg = 0, length = 0, offset = 0, port = 0, address = 0;
    if (napi_create_object(env, &object) != 0 ||
        napi_create_string_utf8(env, "payload", (size_t)-1, &msg) != 0 ||
        napi_create_int32(env, (int32_t)index, &length) != 0 ||
        napi_create_int32(env, 0, &offset) != 0 ||
        napi_create_int32(env, 4433, &port) != 0 ||
        napi_create_string_utf8(env, "127.0.0.1", (size_t)-1, &address) != 0 ||
        napi_set_named_property(env, object, "msg", msg) != 0 ||
        napi_set_named_property(env, object, "length", length) != 0 ||
        napi_set_named_property(env, object, "offset", offset) != 0 ||
        napi_set_named_property(env, object, "port", port) != 0 ||
        napi_set_named_property(env, object, "address", address) != 0 ||
        napi_set_element(env, list, index, object) != 0) {
      return 0;
    }
  }
  return list;
}

NAPI_EXPORT int32_t node_api_module_get_api_version_v1(void) { return 8; }

NAPI_EXPORT napi_value napi_register_module_v1(napi_env env, napi_value exports) {
  static const struct { const char* name; napi_callback callback; } functions[] = {
      {"setNamed", set_named},
      {"setNamedStatus", set_named_status},
      {"getNamed", get_named},
      {"hasNamed", has_named},
      {"make", make},
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
