/* A minimal Node-API addon for the WASI static-addon fixture
 * (scripts/wasi_addon/run.py). Built twice: every global symbol, entry
 * included, is defined by both copies, which must stay isolated. */
#include <stddef.h>
#include <stdint.h>

typedef struct napi_env__ *napi_env;
typedef struct napi_value__ *napi_value;
typedef struct napi_callback_info__ *napi_callback_info;
typedef int napi_status;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);

/* node_api.h declares its imports this way under __wasm__. */
#define NAPI_IMPORT __attribute__((__import_module__("napi")))
NAPI_IMPORT napi_status napi_create_string_utf8(napi_env, const char *, size_t, napi_value *);
NAPI_IMPORT napi_status napi_create_function(napi_env, const char *, size_t, napi_callback, void *,
                                             napi_value *);
NAPI_IMPORT napi_status napi_set_named_property(napi_env, napi_value, const char *, napi_value);
NAPI_IMPORT napi_status napi_create_arraybuffer(napi_env, size_t, void **, napi_value *);
NAPI_IMPORT napi_status napi_create_typedarray(napi_env, int, size_t, napi_value, size_t,
                                               napi_value *);

#define NAPI_UINT8_ARRAY 1

/* Same name, different answer in each copy. */
const char *addon_name(void) { return ADDON_NAME; }

static napi_value string(napi_env env, const char *text) {
  napi_value value;
  napi_create_string_utf8(env, text, SIZE_MAX, &value);
  return value;
}

static napi_value hello(napi_env env, napi_callback_info info) {
  (void)info;
  return string(env, "hello from " ADDON_NAME);
}

static napi_value name(napi_env env, napi_callback_info info) {
  (void)info;
  return string(env, addon_name());
}

static napi_value bytes(napi_env env, napi_callback_info info) {
  (void)info;
  void *data;
  napi_value buffer, array;
  napi_create_arraybuffer(env, 3, &data, &buffer);
  uint8_t *out = data;
  out[0] = 7;
  out[1] = 8;
  out[2] = 9;
  napi_create_typedarray(env, NAPI_UINT8_ARRAY, 3, buffer, 0, &array);
  return array;
}

static void method(napi_env env, napi_value exports, const char *key, napi_callback callback) {
  napi_value fn;
  napi_create_function(env, key, SIZE_MAX, callback, NULL, &fn);
  napi_set_named_property(env, exports, key, fn);
}

__attribute__((visibility("default"))) int32_t node_api_module_get_api_version_v1(void) {
  return 8;
}

__attribute__((visibility("default"))) napi_value napi_register_wasm_v1(napi_env env,
                                                                       napi_value exports) {
  method(env, exports, "hello", hello);
  method(env, exports, "name", name);
  method(env, exports, "bytes", bytes);
  return exports;
}
