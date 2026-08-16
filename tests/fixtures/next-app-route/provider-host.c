/*
 * dlopen host for the production Next App Route dylib gate
 * (tests/test_next_app_route_dylib.sh).
 *
 * Deliberately C, not Rust (#8205). A Rust executable carries rustc's
 * allocator shim (`__rust_alloc` / `__rust_dealloc` / ...), backed by the
 * System allocator. The stdlib provider image imports those same shim symbols
 * and expects the runtime image's mimalloc-backed definitions; when the main
 * executable also defines them, a flat lookup binds the stdlib image to the
 * host's shim, and the first cross-image `Vec` drop frees a mimalloc pointer
 * with libsystem `free()` and aborts. A C host defines no Rust shim, so the
 * only definitions in the process are the runtime image's.
 *
 * Load order and flags are the contract the gate asserts: both providers are
 * process-global and eagerly relocated before the app; the app itself is
 * loaded RTLD_LOCAL with eager relocation, so an unresolved Perry ABI symbol
 * fails at load time rather than on the first request that reaches it.
 */
#include <dlfcn.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef void (*void_fn)(void);
typedef int (*tick_fn)(void);
typedef size_t (*probe_fn)(void);

static void *open_image(const char *path, int mode) {
  void *handle = dlopen(path, mode);
  if (handle == NULL) {
    fprintf(stderr, "dlopen failed: %s: %s\n", path, dlerror());
    exit(1);
  }
  return handle;
}

static void *symbol(void *handle, const char *name) {
  dlerror();
  void *address = dlsym(handle, name);
  const char *error = dlerror();
  if (address == NULL || error != NULL) {
    fprintf(stderr, "dlsym failed: %s: %s\n", name,
            error != NULL ? error : "null address");
    exit(1);
  }
  return address;
}

int main(int argc, char **argv) {
  if (argc != 4) {
    fprintf(stderr, "usage: provider-host runtime stdlib app\n");
    return 1;
  }

  void *runtime = open_image(argv[1], RTLD_NOW | RTLD_GLOBAL);
  void *stdlib = open_image(argv[2], RTLD_NOW | RTLD_GLOBAL);
  void *app = open_image(argv[3], RTLD_NOW | RTLD_LOCAL);

  void_fn gc_init = (void_fn)symbol(runtime, "js_gc_init");
  /* The stdlib provider must bind its stateful runtime calls to the runtime
     image the host loaded, not to a private runtime copy of its own. */
  probe_fn provider_probe =
      (probe_fn)symbol(stdlib, "next_app_route_provider_runtime_probe");
  if (provider_probe() != (size_t)(uintptr_t)gc_init) {
    fprintf(stderr, "stdlib provider is bound to a different runtime image\n");
    return 1;
  }

  void_fn module_init = (void_fn)symbol(app, "perry_module_init");
  tick_fn run_microtasks =
      (tick_fn)symbol(runtime, "js_promise_run_microtasks_event_loop");
  tick_fn timer_tick = (tick_fn)symbol(runtime, "js_timer_tick");
  tick_fn callback_timer_tick =
      (tick_fn)symbol(runtime, "js_callback_timer_tick");
  tick_fn interval_timer_tick =
      (tick_fn)symbol(runtime, "js_interval_timer_tick");
  void_fn run_stdlib_pump = (void_fn)symbol(runtime, "js_run_stdlib_pump");
  void_fn wait_for_event = (void_fn)symbol(runtime, "js_wait_for_event");

  gc_init();
  module_init();
  for (;;) {
    run_microtasks();
    timer_tick();
    callback_timer_tick();
    interval_timer_tick();
    run_stdlib_pump();
    wait_for_event();
  }
}
