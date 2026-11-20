#include <stdint.h>
#include <string.h>

extern void js_object_set_field_by_name(void *, const void *, double);

void js_cxnapi_write(double object, double key, double value) {
  uint64_t object_bits, key_bits;
  memcpy(&object_bits, &object, sizeof object_bits);
  memcpy(&key_bits, &key, sizeof key_bits);
  js_object_set_field_by_name(
      (void *)(uintptr_t)(object_bits & UINT64_C(0x0000ffffffffffff)),
      (void *)(uintptr_t)(key_bits & UINT64_C(0x0000ffffffffffff)), value);
}
