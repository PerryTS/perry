#include <stdint.h>
#include <stddef.h>

#if defined(PERRY_FIXTURE_DIAGNOSTICS)
#include <stdio.h>
#endif

#if defined(_WIN32)
#define NAPI_IMPORT __declspec(dllimport)
#define NAPI_EXPORT __declspec(dllexport)
#else
#define NAPI_IMPORT __attribute__((visibility("default")))
#define NAPI_EXPORT __attribute__((visibility("default")))
#endif

typedef void* napi_env;
typedef void* napi_value;
typedef void* napi_callback_info;
typedef int32_t napi_status;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);

NAPI_IMPORT napi_status napi_create_int32(napi_env, int32_t, napi_value*);
NAPI_IMPORT napi_status napi_create_function(
    napi_env, const char*, size_t, napi_callback, void*, napi_value*);
NAPI_IMPORT napi_status napi_get_cb_info(
    napi_env, napi_callback_info, size_t*, napi_value*, napi_value*, void**);
NAPI_IMPORT napi_status napi_get_value_int32(napi_env, napi_value, int32_t*);
NAPI_IMPORT napi_status napi_set_named_property(
    napi_env, napi_value, const char*, napi_value);

static napi_value add(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {0, 0};
  int32_t left = 0;
  int32_t right = 0;
  napi_value result = 0;
  if (napi_get_cb_info(env, info, &argc, argv, 0, 0) != 0 || argc != 2 ||
      napi_get_value_int32(env, argv[0], &left) != 0 ||
      napi_get_value_int32(env, argv[1], &right) != 0 ||
      napi_create_int32(env, left + right, &result) != 0) {
    return 0;
  }
  return result;
}

NAPI_EXPORT int32_t node_api_module_get_api_version_v1(void) { return 8; }

NAPI_EXPORT napi_value napi_register_module_v1(napi_env env, napi_value exports) {
  napi_value answer = 0;
  napi_value add_function = 0;
  int32_t create_answer = napi_create_int32(env, 8523, &answer);
  int32_t set_answer = create_answer == 0
                           ? napi_set_named_property(env, exports, "answer", answer)
                           : -1;
  int32_t create_add = set_answer == 0
                           ? napi_create_function(env, "add", 3, add, 0, &add_function)
                           : -1;
  int32_t set_add = create_add == 0
                        ? napi_set_named_property(env, exports, "add", add_function)
                        : -1;
#if defined(PERRY_FIXTURE_DIAGNOSTICS)
  fprintf(stderr, "fixture statuses: %d %d %d %d\n", create_answer, set_answer,
          create_add, set_add);
#endif
  if (create_answer != 0 || set_answer != 0 || create_add != 0 || set_add != 0) {
    return 0;
  }
  return exports;
}
