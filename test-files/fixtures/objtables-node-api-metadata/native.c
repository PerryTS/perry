#include <stdint.h>
#include <stddef.h>
typedef void *napi_env;
typedef void *napi_value;
typedef void *napi_callback_info;
typedef struct { uint64_t lower, upper; } napi_type_tag;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);
extern int32_t napi_create_int32(napi_env, int32_t, napi_value *);
extern int32_t napi_create_function(napi_env, const char *, size_t, napi_callback, void *, napi_value *);
extern int32_t napi_get_cb_info(napi_env, napi_callback_info, size_t *, napi_value *, napi_value *, void **);
extern int32_t napi_set_named_property(napi_env, napi_value, const char *, napi_value);
extern int32_t napi_wrap(napi_env, napi_value, void *, void *, void *, void *);
extern int32_t napi_unwrap(napi_env, napi_value, void **);
extern int32_t napi_type_tag_object(napi_env, napi_value, const napi_type_tag *);
extern int32_t napi_check_object_type_tag(napi_env, napi_value, const napi_type_tag *, unsigned char *);
static const napi_type_tag TAG = {11, 13};
static napi_value attach(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value arg = 0, result = 0;
    if (napi_get_cb_info(env, info, &argc, &arg, 0, 0) || argc != 1) return 0;
    if (napi_type_tag_object(env, arg, &TAG) || napi_wrap(env, arg, (void *)(uintptr_t)37, 0, 0, 0)) return 0;
    napi_create_int32(env, 1, &result);
    return result;
}
static napi_value read(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value arg = 0, result = 0;
    void *data = 0;
    unsigned char tagged = 0;
    if (napi_get_cb_info(env, info, &argc, &arg, 0, 0) || argc != 1) return 0;
    if (napi_check_object_type_tag(env, arg, &TAG, &tagged) || !tagged || napi_unwrap(env, arg, &data)) data = 0;
    napi_create_int32(env, (int32_t)(uintptr_t)data, &result);
    return result;
}
__attribute__((visibility("default"))) int32_t node_api_module_get_api_version_v1(void) { return 8; }
__attribute__((visibility("default"))) napi_value napi_register_module_v1(napi_env env, napi_value exports) {
    napi_value fn = 0;
    napi_create_function(env, "attach", 6, attach, 0, &fn);
    napi_set_named_property(env, exports, "attach", fn);
    napi_create_function(env, "read", 4, read, 0, &fn);
    napi_set_named_property(env, exports, "read", fn);
    return exports;
}
