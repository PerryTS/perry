#include <dlfcn.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

typedef void (*module_init_fn)(void);
typedef int (*poll_fn)(void);
typedef void (*wait_fn)(void);

static void *load_image(const char *path, int flags) {
  void *image = dlopen(path, flags);
  if (image == NULL) {
    fprintf(stderr, "dlopen(%s): %s\n", path, dlerror());
    exit(1);
  }
  return image;
}

static void *load_symbol(void *image, const char *name) {
  dlerror();
  void *symbol = dlsym(image, name);
  const char *error = dlerror();
  if (error != NULL) {
    fprintf(stderr, "dlsym(%s): %s\n", name, error);
    exit(1);
  }
  return symbol;
}

int main(int argc, char **argv) {
  if (argc != 4) {
    fprintf(stderr, "usage: provider-host RUNTIME STDLIB APP\n");
    return 2;
  }

  /* Runtime and stdlib are process-global providers. Runtime is lazy because
     its event-pump surface calls back into stdlib; stdlib then resolves its
     own runtime references from the already-global runtime image. */
  void *runtime = load_image(argv[1], RTLD_LAZY | RTLD_GLOBAL);
  load_image(argv[2], RTLD_NOW | RTLD_GLOBAL);

  /* The production application must pass eager relocation after both ABI
     providers are present. No unresolved Perry ABI is deferred to traffic. */
  void *app = load_image(argv[3], RTLD_NOW | RTLD_LOCAL);
  module_init_fn initialize = (module_init_fn)load_symbol(app, "perry_module_init");
  poll_fn poll = (poll_fn)load_symbol(runtime, "perry_poll");
  wait_fn wait_for_event =
      (wait_fn)load_symbol(runtime, "js_wait_for_event");

  initialize();
  for (;;) {
    int progressed = poll();
    if (!progressed) {
      /* This is also the single-thread stdlib reactor driver. A host-side
         sleep would leave accepted sockets and native timers unpolled. */
      wait_for_event();
    }
  }
}
