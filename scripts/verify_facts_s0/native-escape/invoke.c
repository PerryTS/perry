#include <stddef.h>
typedef void *napi_env;
typedef void *napi_value;
typedef void *napi_callback_info;
typedef int napi_status;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);
extern napi_status napi_get_cb_info(napi_env, napi_callback_info, size_t *, napi_value *, napi_value *, void **);
extern napi_status napi_get_undefined(napi_env, napi_value *);
extern napi_status napi_call_function(napi_env, napi_value, napi_value, size_t, const napi_value *, napi_value *);
extern napi_status napi_create_function(napi_env, const char *, size_t, napi_callback, void *, napi_value *);
extern napi_status napi_set_named_property(napi_env, napi_value, const char *, napi_value);
static napi_value invoke(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value callback, receiver, result;
    napi_get_cb_info(env, info, &argc, &callback, NULL, NULL);
    napi_get_undefined(env, &receiver);
    napi_call_function(env, receiver, callback, 0, NULL, &result);
    return result;
}
napi_value napi_register_module_v1(napi_env env, napi_value exports) {
    napi_value fn;
    napi_create_function(env, "invoke", (size_t)-1, invoke, NULL, &fn);
    napi_set_named_property(env, exports, "invoke", fn);
    return exports;
}
