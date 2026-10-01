#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>

typedef void *napi_env;
typedef void *napi_value;
typedef void *napi_callback_info;
typedef int32_t napi_status;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);
typedef void (*napi_finalize)(napi_env, void *, void *);
extern napi_status napi_create_external_buffer(napi_env, size_t, void *, napi_finalize, void *, napi_value *);
extern napi_status napi_create_function(napi_env, const char *, size_t, napi_callback, void *, napi_value *);
extern napi_status napi_set_named_property(napi_env, napi_value, const char *, napi_value);
extern napi_status napi_create_int32(napi_env, int32_t, napi_value *);
static int32_t released;
static void release_bytes(napi_env env, void *bytes, void *hint) {
    (void)env; (void)hint;
    free(bytes);
    released++;
}
static napi_value create(napi_env env, napi_callback_info info) {
    (void)info;
    unsigned char *bytes = malloc(3);
    if (!bytes) return 0;
    bytes[0] = 65; bytes[1] = 66; bytes[2] = 67;
    napi_value value = 0;
    if (napi_create_external_buffer(env, 3, bytes, release_bytes, 0, &value)) {
        free(bytes);
        return 0;
    }
    return value;
}
static napi_value count(napi_env env, napi_callback_info info) {
    (void)info;
    napi_value value = 0;
    napi_create_int32(env, released, &value);
    return value;
}
__attribute__((visibility("default"))) int32_t node_api_module_get_api_version_v1(void) { return 8; }
__attribute__((visibility("default"))) napi_value napi_register_module_v1(napi_env env, napi_value exports) {
    napi_value fn = 0;
    if (napi_create_function(env, "create", 6, create, 0, &fn) ||
        napi_set_named_property(env, exports, "create", fn) ||
        napi_create_function(env, "released", 8, count, 0, &fn) ||
        napi_set_named_property(env, exports, "released", fn)) return 0;
    return exports;
}
