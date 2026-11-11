/* An addon with a symbol the host never defines, on a path it never calls.
 *
 * Prebuilt addons do this: the published @fails-components/webtransport
 * binary references ICU 74 functions (u_errorName_74, ...) that a host built
 * against another ICU major does not export, on IDNA paths an echo server
 * never reaches. Node loads it because process.dlopen binds lazily by
 * default (RTLD_LAZY); an explicit RTLD_NOW resolves everything at load and
 * is rejected. `ping` works; `unused` would call the missing function.
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
NAPI_IMPORT napi_status napi_create_int32(napi_env, int32_t, napi_value*);
NAPI_IMPORT napi_status napi_set_named_property(napi_env, napi_value, const char*, napi_value);

/* Defined by no library the host or the addon links against. */
extern int32_t perry_fixture_never_defined(int32_t);

static napi_value ping(napi_env env, napi_callback_info info) {
  napi_value value = 0;
  (void)info;
  if (napi_create_int32(env, 42, &value) != 0) return 0;
  return value;
}

static napi_value unused(napi_env env, napi_callback_info info) {
  napi_value value = 0;
  (void)info;
  if (napi_create_int32(env, perry_fixture_never_defined(1), &value) != 0) return 0;
  return value;
}

NAPI_EXPORT napi_value napi_register_module_v1(napi_env env, napi_value exports) {
  static const struct { const char* name; napi_callback callback; } functions[] = {
      {"ping", ping},
      {"unused", unused},
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
