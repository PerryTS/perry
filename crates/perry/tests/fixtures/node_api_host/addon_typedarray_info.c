/* napi_get_typedarray_info on typed arrays, Buffers, offset views, detached
 * and out-of-bounds views.
 *
 * `info(value)` asks for every output at once, then for the data pointer
 * without the ArrayBuffer and for the byte offset alone, and reports what it
 * got: element type, length, byte offset, whether data is NULL, data minus
 * the backing ArrayBuffer data, the first element byte, and whether the
 * partial calls agree. A failing status is reported as its number.
 * `buffer(value)` returns the reported ArrayBuffer. `create(arraybuffer,
 * type)` returns napi_create_typedarray's view of two elements at offset 0,
 * or its status number. `bufinfo(value)` reports napi_is_buffer and
 * napi_get_buffer_info (status, length, data, first byte). `external()`
 * returns napi_create_external_buffer's value over four static bytes.
 * Node's answers are the expected transcript.
 */
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>

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
NAPI_IMPORT napi_status napi_create_string_utf8(napi_env, const char*, size_t, napi_value*);
NAPI_IMPORT napi_status napi_set_named_property(napi_env, napi_value, const char*, napi_value);
NAPI_IMPORT napi_status napi_get_typedarray_info(
    napi_env, napi_value, int*, size_t*, void**, napi_value*, size_t*);
NAPI_IMPORT napi_status napi_get_arraybuffer_info(napi_env, napi_value, void**, size_t*);
NAPI_IMPORT napi_status napi_get_undefined(napi_env, napi_value*);
NAPI_IMPORT napi_status napi_get_value_int32(napi_env, napi_value, int32_t*);
NAPI_IMPORT napi_status napi_create_int32(napi_env, int32_t, napi_value*);
NAPI_IMPORT napi_status napi_create_typedarray(
    napi_env, int, size_t, napi_value, size_t, napi_value*);
NAPI_IMPORT napi_status napi_is_exception_pending(napi_env, _Bool*);
NAPI_IMPORT napi_status napi_is_buffer(napi_env, napi_value, _Bool*);
NAPI_IMPORT napi_status napi_get_buffer_info(napi_env, napi_value, void**, size_t*);
NAPI_IMPORT napi_status napi_create_external_buffer(
    napi_env, size_t, void*, void (*)(napi_env, void*, void*), void*, napi_value*);
NAPI_IMPORT napi_status napi_get_and_clear_last_exception(napi_env, napi_value*);

static napi_value text(napi_env env, const char* s) {
  napi_value out = 0;
  napi_create_string_utf8(env, s, (size_t)-1, &out);
  return out;
}

static napi_value info(napi_env env, napi_callback_info cb) {
  size_t argc = 1;
  napi_value argv[1] = {0};
  napi_get_cb_info(env, cb, &argc, argv, 0, 0);
  char out[256];
  int type = -1;
  size_t length = 777, offset = 777;
  void* data = (void*)1;
  napi_value ab = 0;
  napi_status status = napi_get_typedarray_info(env, argv[0], &type, &length, &data, &ab, &offset);
  if (status != 0) {
    snprintf(out, sizeof out, "status=%d", (int)status);
    return text(env, out);
  }
  void* ab_data = (void*)1;
  size_t ab_length = 777;
  napi_status ab_status = napi_get_arraybuffer_info(env, ab, &ab_data, &ab_length);
  void* data2 = (void*)1;
  size_t length2 = 777;
  napi_get_typedarray_info(env, argv[0], 0, &length2, &data2, 0, 0);
  size_t offset3 = 777;
  napi_get_typedarray_info(env, argv[0], 0, 0, 0, 0, &offset3);
  long delta = (data && ab_data) ? (long)((char*)data - (char*)ab_data) : -1;
  int first = (data && length) ? ((unsigned char*)data)[0] : -1;
  snprintf(out, sizeof out,
           "type=%d length=%zu offset=%zu data=%s delta=%ld first=%d ab=%d/%s/%zu "
           "nobuf_same=%d offset_only=%zu",
           type, length, offset, data ? "ptr" : "null", delta, first, (int)ab_status,
           ab_data ? "ptr" : "null", ab_length, data2 == data && length2 == length, offset3);
  return text(env, out);
}

static napi_value buffer(napi_env env, napi_callback_info cb) {
  size_t argc = 1;
  napi_value argv[1] = {0};
  napi_get_cb_info(env, cb, &argc, argv, 0, 0);
  napi_value ab = 0;
  if (napi_get_typedarray_info(env, argv[0], 0, 0, 0, &ab, 0) != 0) {
    napi_get_undefined(env, &ab);
  }
  return ab;
}

static napi_value create(napi_env env, napi_callback_info cb) {
  size_t argc = 2;
  napi_value argv[2] = {0, 0};
  napi_get_cb_info(env, cb, &argc, argv, 0, 0);
  int32_t type = -1;
  napi_get_value_int32(env, argv[1], &type);
  napi_value out = 0;
  napi_status status = napi_create_typedarray(env, type, 2, argv[0], 0, &out);
  if (status != 0) {
    _Bool pending = 0;
    napi_value ignored = 0;
    if (napi_is_exception_pending(env, &pending) == 0 && pending) {
      napi_get_and_clear_last_exception(env, &ignored);
    }
    napi_create_int32(env, status, &out);
  }
  return out;
}

static napi_value bufinfo(napi_env env, napi_callback_info cb) {
  size_t argc = 1;
  napi_value argv[1] = {0};
  napi_get_cb_info(env, cb, &argc, argv, 0, 0);
  char out[160];
  _Bool is = 0;
  napi_status is_status = napi_is_buffer(env, argv[0], &is);
  void* data = (void*)1;
  size_t length = 777;
  napi_status status = napi_get_buffer_info(env, argv[0], &data, &length);
  if (status != 0) {
    snprintf(out, sizeof out, "is=%d/%d status=%d", (int)is_status, (int)is, (int)status);
  } else {
    int first = (data && length) ? ((unsigned char*)data)[0] : -1;
    snprintf(out, sizeof out, "is=%d/%d length=%zu data=%s first=%d", (int)is_status, (int)is,
             length, data ? "ptr" : "null", first);
  }
  return text(env, out);
}

static unsigned char external_bytes[4] = {9, 8, 7, 6};

static napi_value external(napi_env env, napi_callback_info cb) {
  (void)cb;
  napi_value out = 0;
  napi_status status =
      napi_create_external_buffer(env, sizeof external_bytes, external_bytes, 0, 0, &out);
  if (status != 0) {
    napi_create_int32(env, status, &out);
  }
  return out;
}

NAPI_EXPORT int32_t node_api_module_get_api_version_v1(void) { return 8; }

NAPI_EXPORT napi_value napi_register_module_v1(napi_env env, napi_value exports) {
  static const struct { const char* name; napi_callback callback; } functions[] = {
      {"info", info},
      {"buffer", buffer},
      {"create", create},
      {"bufinfo", bufinfo},
      {"external", external},
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
