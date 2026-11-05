/* Linked only on request (`-u perry_net_host`, see `perry_wasi_net.c`): the
 * module then imports the three `perry_net` functions. Each returns a WASI
 * errno and writes its result through a u32 out-parameter. */
#if defined(__wasip1__)
#include <stdint.h>

#define IMPORT(name) __attribute__((__import_module__("perry_net"), __import_name__(name)))

IMPORT("tcp_connect") extern uint32_t perry_net_import_tcp_connect(const char *addr, uint32_t len,
                                                                   uint32_t *fd);
IMPORT("sock_error") extern uint32_t perry_net_import_sock_error(uint32_t fd, uint32_t *err);
IMPORT("resolve") extern uint32_t perry_net_import_resolve(const char *host, uint32_t len,
                                                           uint32_t family, uint32_t *fd);

/* The marker the compiler asks for; it keeps this member, and so the imports. */
int perry_net_host(void) { return 1; }

int perry_net_tcp_connect(const char *addr, unsigned len, unsigned *fd) {
    return (int)perry_net_import_tcp_connect(addr, len, fd);
}

int perry_net_sock_error(unsigned fd, unsigned *err) {
    return (int)perry_net_import_sock_error(fd, err);
}

int perry_net_resolve(const char *host, unsigned len, unsigned family, unsigned *fd) {
    return (int)perry_net_import_resolve(host, len, family, fd);
}
#endif
