#include <stdint.h>
#include <stddef.h>
typedef void *napi_env;
typedef void *napi_value;
typedef void *napi_callback_info;
typedef int32_t napi_status;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);
extern napi_status napi_create_double(napi_env, double, napi_value *);
extern napi_status napi_create_function(napi_env, const char *, size_t, napi_callback, void *, napi_value *);
extern napi_status napi_set_named_property(napi_env, napi_value, const char *, napi_value);
static napi_value negative_nan(napi_env env, napi_callback_info info) {
    (void)info;
    union { uint64_t bits; double value; } number = { .bits = UINT64_C(0xfff9000012345678) };
    napi_value result = 0;
    if (napi_create_double(env, number.value, &result)) return 0;
    return result;
}
__attribute__((visibility("default"))) napi_value napi_register_module_v1(napi_env env, napi_value exports) {
    napi_value function = 0;
    if (napi_create_function(env, "negativeNaN", 11, negative_nan, 0, &function) ||
        napi_set_named_property(env, exports, "negativeNaN", function)) return 0;
    return exports;
}
