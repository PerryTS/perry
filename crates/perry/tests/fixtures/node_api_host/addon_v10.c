/* Node-API 9/10 fixture for #10456 and #10461.
 *
 * Built three times with -DFIXTURE_API_VERSION=10, 8 and 11. It declares its
 * own prototypes so neither Node's headers nor a JavaScript engine are needed
 * to build it, and every entry point it imports must resolve from the host.
 */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#define NAPI_IMPORT __attribute__((visibility("default")))
#define NAPI_EXPORT __attribute__((visibility("default")))
#define NAPI_AUTO_LENGTH SIZE_MAX

#ifndef FIXTURE_API_VERSION
#define FIXTURE_API_VERSION 10
#endif

typedef void* napi_env;
typedef void* napi_value;
typedef void* napi_callback_info;
typedef void* napi_async_work;
typedef void* napi_threadsafe_function;
typedef int32_t napi_status;
typedef int32_t napi_valuetype;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);
typedef void (*napi_finalize)(napi_env, void*, void*);
typedef void (*napi_async_execute_callback)(napi_env, void*);
typedef void (*napi_async_complete_callback)(napi_env, napi_status, void*);
typedef void (*napi_threadsafe_function_call_js)(napi_env, napi_value, void*, void*);

NAPI_IMPORT napi_status napi_create_function(
    napi_env, const char*, size_t, napi_callback, void*, napi_value*);
NAPI_IMPORT napi_status napi_get_cb_info(
    napi_env, napi_callback_info, size_t*, napi_value*, napi_value*, void**);
NAPI_IMPORT napi_status napi_set_named_property(napi_env, napi_value, const char*, napi_value);
NAPI_IMPORT napi_status napi_set_property(napi_env, napi_value, napi_value, napi_value);
NAPI_IMPORT napi_status napi_create_object(napi_env, napi_value*);
NAPI_IMPORT napi_status napi_create_int32(napi_env, int32_t, napi_value*);
NAPI_IMPORT napi_status napi_create_uint32(napi_env, uint32_t, napi_value*);
NAPI_IMPORT napi_status napi_create_string_utf8(napi_env, const char*, size_t, napi_value*);
NAPI_IMPORT napi_status napi_get_value_string_utf8(napi_env, napi_value, char*, size_t, size_t*);
NAPI_IMPORT napi_status napi_get_value_double(napi_env, napi_value, double*);
NAPI_IMPORT napi_status napi_get_value_uint32(napi_env, napi_value, uint32_t*);
NAPI_IMPORT napi_status napi_get_boolean(napi_env, bool, napi_value*);
NAPI_IMPORT napi_status napi_get_null(napi_env, napi_value*);
NAPI_IMPORT napi_status napi_typeof(napi_env, napi_value, napi_valuetype*);
NAPI_IMPORT napi_status napi_new_instance(napi_env, napi_value, size_t, const napi_value*, napi_value*);
NAPI_IMPORT napi_status napi_get_version(napi_env, uint32_t*);
NAPI_IMPORT napi_status napi_throw_error(napi_env, const char*, const char*);
NAPI_IMPORT napi_status napi_create_async_work(
    napi_env, napi_value, napi_value, napi_async_execute_callback,
    napi_async_complete_callback, void*, napi_async_work*);
NAPI_IMPORT napi_status napi_queue_async_work(napi_env, napi_async_work);
NAPI_IMPORT napi_status napi_delete_async_work(napi_env, napi_async_work);
NAPI_IMPORT napi_status napi_create_threadsafe_function(
    napi_env, napi_value, napi_value, napi_value, size_t, size_t, void*, napi_finalize,
    void*, napi_threadsafe_function_call_js, napi_threadsafe_function*);
NAPI_IMPORT napi_status napi_call_threadsafe_function(napi_threadsafe_function, void*, int32_t);
NAPI_IMPORT napi_status napi_release_threadsafe_function(napi_threadsafe_function, int32_t);

/* Node-API 9 */
NAPI_IMPORT napi_status node_api_symbol_for(napi_env, const char*, size_t, napi_value*);
NAPI_IMPORT napi_status node_api_create_syntax_error(napi_env, napi_value, napi_value, napi_value*);
NAPI_IMPORT napi_status node_api_throw_syntax_error(napi_env, const char*, const char*);
NAPI_IMPORT napi_status node_api_get_module_file_name(napi_env, const char**);

/* Node-API 10 */
NAPI_IMPORT napi_status node_api_create_external_string_latin1(
    napi_env, char*, size_t, napi_finalize, void*, napi_value*, bool*);
NAPI_IMPORT napi_status node_api_create_external_string_utf16(
    napi_env, uint16_t*, size_t, napi_finalize, void*, napi_value*, bool*);
NAPI_IMPORT napi_status node_api_create_property_key_latin1(napi_env, const char*, size_t, napi_value*);
NAPI_IMPORT napi_status node_api_create_property_key_utf8(napi_env, const char*, size_t, napi_value*);
NAPI_IMPORT napi_status node_api_create_property_key_utf16(napi_env, const uint16_t*, size_t, napi_value*);
NAPI_IMPORT napi_status node_api_create_buffer_from_arraybuffer(
    napi_env, napi_value, size_t, size_t, napi_value*);

static napi_value arg0(napi_env env, napi_callback_info info, napi_value* extra) {
  size_t argc = 3;
  napi_value argv[3] = {0, 0, 0};
  napi_get_cb_info(env, info, &argc, argv, 0, 0);
  if (extra) {
    extra[0] = argv[1];
    extra[1] = argv[2];
  }
  return argv[0];
}

static napi_value string(napi_env env, const char* text) {
  napi_value value = 0;
  napi_create_string_utf8(env, text, NAPI_AUTO_LENGTH, &value);
  return value;
}

static napi_value TypeOf(napi_env env, napi_callback_info info) {
  static const char* names[] = {"undefined", "null",     "boolean",  "number", "string",
                                "symbol",    "object",   "function", "external", "bigint"};
  napi_valuetype type = 0;
  if (napi_typeof(env, arg0(env, info, 0), &type) != 0 || type < 0 || type > 9) {
    return string(env, "napi_typeof failed");
  }
  return string(env, names[type]);
}

static napi_value IntTypeOf(napi_env env, napi_callback_info info) {
  uint32_t input = 0;
  napi_value created = 0;
  napi_valuetype type = 0;
  napi_get_value_uint32(env, arg0(env, info, 0), &input);
  napi_create_int32(env, (int32_t)input, &created);
  napi_typeof(env, created, &type);
  return string(env, type == 3 ? "number" : "not-a-number");
}

static napi_value NumberStatus(napi_env env, napi_callback_info info) {
  double ignored = 0;
  napi_value result = 0;
  napi_create_int32(env, napi_get_value_double(env, arg0(env, info, 0), &ignored), &result);
  return result;
}

static napi_value Construct(napi_env env, napi_callback_info info) {
  napi_value result = 0;
  if (napi_new_instance(env, arg0(env, info, 0), 0, 0, &result) != 0) {
    napi_get_null(env, &result);
  }
  return result;
}

static napi_value Version(napi_env env, napi_callback_info info) {
  uint32_t version = 0;
  napi_value result = 0;
  (void)info;
  napi_get_version(env, &version);
  napi_create_uint32(env, version, &result);
  return result;
}

static napi_value SymbolFor(napi_env env, napi_callback_info info) {
  char key[64];
  size_t length = 0;
  napi_value result = 0;
  napi_get_value_string_utf8(env, arg0(env, info, 0), key, sizeof key, &length);
  node_api_symbol_for(env, key, length, &result);
  return result;
}

static napi_value SyntaxError(napi_env env, napi_callback_info info) {
  napi_value rest[2] = {0, 0};
  napi_value code = arg0(env, info, rest);
  napi_value result = 0;
  node_api_create_syntax_error(env, code, rest[0], &result);
  return result;
}

static napi_value ThrowSyntax(napi_env env, napi_callback_info info) {
  (void)info;
  node_api_throw_syntax_error(env, "ERR_FIXTURE_SYNTAX", "fixture syntax");
  return 0;
}

static napi_value ModuleFile(napi_env env, napi_callback_info info) {
  const char* file = 0;
  (void)info;
  if (node_api_get_module_file_name(env, &file) != 0 || file == 0) {
    return string(env, "node_api_get_module_file_name failed");
  }
  return string(env, file);
}

static int finalized_strings = 0;
static bool contract_held = true;
static char latin1_storage[] = "external latin1 \xe9";
static uint16_t utf16_storage[] = {'u', 't', 'f', '1', '6', ' ', 0xd83d, 0xde00, 0};

static void count_string_finalizer(napi_env env, void* data, void* hint) {
  (void)env;
  (void)data;
  (void)hint;
  finalized_strings++;
}

/* Node may adopt the storage (finalizer later) or copy it (finalizer already
 * ran); either way `copied` must describe what happened. */
static void check_contract(bool copied, int before) {
  if (copied != (finalized_strings == before + 1)) {
    contract_held = false;
  }
}

static napi_value ExternalLatin1(napi_env env, napi_callback_info info) {
  napi_value result = 0;
  bool copied = false;
  int before = finalized_strings;
  (void)info;
  if (node_api_create_external_string_latin1(env, latin1_storage, NAPI_AUTO_LENGTH,
                                             count_string_finalizer, 0, &result,
                                             &copied) != 0) {
    return string(env, "external latin1 failed");
  }
  check_contract(copied, before);
  return result;
}

static napi_value ExternalUtf16(napi_env env, napi_callback_info info) {
  napi_value result = 0;
  bool copied = false;
  int before = finalized_strings;
  (void)info;
  if (node_api_create_external_string_utf16(env, utf16_storage, 8, count_string_finalizer, 0,
                                            &result, &copied) != 0) {
    return string(env, "external utf16 failed");
  }
  check_contract(copied, before);
  return result;
}

static napi_value ExternalContract(napi_env env, napi_callback_info info) {
  napi_value result = 0;
  (void)info;
  napi_get_boolean(env, contract_held, &result);
  return result;
}

static napi_value KeysObject(napi_env env, napi_callback_info info) {
  static const uint16_t utf16_key[] = {'u', 't', 'f', '1', '6', 0xe9};
  napi_value object = 0, key = 0, value = 0;
  (void)info;
  napi_create_object(env, &object);
  node_api_create_property_key_latin1(env, "latin1\xe9", NAPI_AUTO_LENGTH, &key);
  napi_create_int32(env, 1, &value);
  napi_set_property(env, object, key, value);
  node_api_create_property_key_utf8(env, "utf8", 4, &key);
  napi_create_int32(env, 2, &value);
  napi_set_property(env, object, key, value);
  node_api_create_property_key_utf16(env, utf16_key, 6, &key);
  napi_create_int32(env, 3, &value);
  napi_set_property(env, object, key, value);
  return object;
}

static napi_value BufferView(napi_env env, napi_callback_info info) {
  napi_value rest[2] = {0, 0};
  napi_value arraybuffer = arg0(env, info, rest);
  double offset = 0, length = 0;
  napi_value result = 0;
  napi_get_value_double(env, rest[0], &offset);
  napi_get_value_double(env, rest[1], &length);
  if (node_api_create_buffer_from_arraybuffer(env, arraybuffer, (size_t)offset, (size_t)length,
                                              &result) != 0) {
    return string(env, "node_api_create_buffer_from_arraybuffer failed");
  }
  return result;
}

static napi_async_work pending_work = 0;

static void work_execute(napi_env env, void* data) {
  (void)env;
  (void)data;
}

static void work_complete(napi_env env, napi_status status, void* data) {
  (void)status;
  (void)data;
  napi_delete_async_work(env, pending_work);
  pending_work = 0;
  napi_throw_error(env, 0, "async work completion failed");
}

static napi_value ThrowFromWork(napi_env env, napi_callback_info info) {
  (void)info;
  if (napi_create_async_work(env, 0, string(env, "fixture"), work_execute, work_complete, 0,
                             &pending_work) != 0 ||
      napi_queue_async_work(env, pending_work) != 0) {
    napi_throw_error(env, 0, "could not queue async work");
  }
  return 0;
}

static void tsfn_call_js(napi_env env, napi_value function, void* context, void* data) {
  (void)function;
  (void)context;
  (void)data;
  if (env) {
    napi_throw_error(env, 0, FIXTURE_API_VERSION >= 10 ? "tsfn callback failed (10)"
                                                      : "tsfn callback failed (8)");
  }
}

static napi_value ThrowFromTsfn(napi_env env, napi_callback_info info) {
  napi_threadsafe_function tsfn = 0;
  (void)info;
  napi_create_threadsafe_function(env, 0, 0, string(env, "fixture"), 0, 1, 0, 0, 0,
                                  tsfn_call_js, &tsfn);
  napi_call_threadsafe_function(tsfn, 0, 0);
  napi_release_threadsafe_function(tsfn, 0);
  return 0;
}

NAPI_EXPORT int32_t node_api_module_get_api_version_v1(void) { return FIXTURE_API_VERSION; }

NAPI_EXPORT napi_value napi_register_module_v1(napi_env env, napi_value exports) {
  static const struct {
    const char* name;
    napi_callback callback;
  } methods[] = {
      {"typeOf", TypeOf},           {"intTypeOf", IntTypeOf},
      {"numberStatus", NumberStatus}, {"construct", Construct},
      {"version", Version},         {"symbolFor", SymbolFor},
      {"syntaxError", SyntaxError}, {"throwSyntax", ThrowSyntax},
      {"moduleFile", ModuleFile},   {"externalLatin1", ExternalLatin1},
      {"externalUtf16", ExternalUtf16}, {"externalContract", ExternalContract},
      {"keysObject", KeysObject},   {"bufferView", BufferView},
      {"throwFromWork", ThrowFromWork}, {"throwFromTsfn", ThrowFromTsfn},
  };
  for (size_t i = 0; i < sizeof methods / sizeof methods[0]; i++) {
    napi_value function = 0;
    if (napi_create_function(env, methods[i].name, NAPI_AUTO_LENGTH, methods[i].callback, 0,
                             &function) != 0 ||
        napi_set_named_property(env, exports, methods[i].name, function) != 0) {
      return 0;
    }
  }
  return exports;
}
