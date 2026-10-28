/* wasi-sdk's libc implements WASIp2 name lookup. Rust std's resolver on
 * this target is still unsupported; keep libc layouts on the C side. */
/* wasm32-wasip1 has no socket creation, connect, bind or name resolution in
 * wasi_snapshot_preview1. Inbound TCP works on listeners the embedder
 * pre-opens (`PERRY_LISTEN_FDS`), through sock_accept, sock_recv, sock_send and
 * sock_shutdown; outbound connects and name resolution report "not supported",
 * so the module imports nothing beyond preview 1. A listener that no host
 * pre-opened reports EADDRNOTAVAIL. Negative results are WASI errno values,
 * as in the WASIp2 arm below. */
#if defined(__wasip1__)
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <wasi/api.h>

#define MAX_LISTENERS 16

/* Listener discovery. The embedder pre-opens listeners and names them in
 * PERRY_LISTEN_FDS (comma separated fd numbers) and, optionally, their bound
 * addresses in PERRY_LISTEN_ADDRS (same order). Without the variable, scan for
 * open stream sockets (a Wasmtime `-S tcplisten` listener; its rights do not
 * include sock_accept, so the file type is the only marker). Each listen()
 * claims the next unclaimed listener in that order; the requested host and
 * port do not select one. */
static int listener_fds[MAX_LISTENERS];
static char listener_addrs[MAX_LISTENERS][64];
static int listener_count = -1, listener_next;

int perry_wasi_socket_nonblock(int fd);

/* The embedder's signal stream (PERRY_SIGNAL_FD), made non-blocking on first
 * use, or -1 when absent, unusable or retired. */
static int signal_fd = -2;
int perry_wasi_signal_fd(void) {
    if (signal_fd != -2) return signal_fd;
    signal_fd = -1;
    const char *v = getenv("PERRY_SIGNAL_FD");
    if (!v || !*v) return signal_fd;
    char *end;
    long fd = strtol(v, &end, 10);
    if (end == v || *end || fd < 0 || fd > 0x7fffffffL) return signal_fd;
    if (perry_wasi_socket_nonblock((int)fd) == 0) signal_fd = (int)fd;
    return signal_fd;
}

/* The stream hit EOF or failed: close it and never hand it out again. */
void perry_wasi_signal_retire(void) {
    int fd = perry_wasi_signal_fd();
    if (fd >= 0) __wasi_fd_close((__wasi_fd_t)fd);
    signal_fd = -1;
}

static void discover_listeners(void) {
    listener_count = 0;
    const char *fds = getenv("PERRY_LISTEN_FDS");
    if (fds && *fds) {
        for (const char *c = fds; *c && listener_count < MAX_LISTENERS;) {
            char *end;
            long fd = strtol(c, &end, 10);
            if (end == c) break;
            listener_fds[listener_count++] = (int)fd;
            c = *end == ',' ? end + 1 : end;
        }
        const char *addrs = getenv("PERRY_LISTEN_ADDRS");
        for (int i = 0; addrs && *addrs && i < listener_count; i++) {
            const char *comma = strchr(addrs, ',');
            size_t len = comma ? (size_t)(comma - addrs) : strlen(addrs);
            if (len < sizeof(listener_addrs[i])) {
                memcpy(listener_addrs[i], addrs, len);
                listener_addrs[i][len] = 0;
            }
            addrs = comma ? comma + 1 : addrs + len;
        }
        return;
    }
    int signal = perry_wasi_signal_fd();
    for (int fd = 3; fd < 64 && listener_count < MAX_LISTENERS; fd++) {
        __wasi_fdstat_t st;
        if (__wasi_fd_fdstat_get((__wasi_fd_t)fd, &st) != 0) continue; /* gaps are legal */
        if (st.fs_filetype == __WASI_FILETYPE_SOCKET_STREAM && fd != signal)
            listener_fds[listener_count++] = fd;
    }
}

/* The signal and stdin streams are not sockets, so `sock_recv` does not apply. */
int perry_wasi_stream_read(int fd, unsigned char *bytes, unsigned len) {
    __wasi_iovec_t iov = {bytes, len};
    __wasi_size_t n;
    __wasi_errno_t e = __wasi_fd_read((__wasi_fd_t)fd, &iov, 1, &n);
    return e ? -(int)e : (int)n;
}

int perry_wasi_resolve(const char *hostname, int family, void *ctx,
                       void (*push)(void *, int, const unsigned char *)) {
    (void)hostname; (void)family; (void)ctx; (void)push;
    return ENOTSUP;
}

/* Sockets the embedder hands out are not created non-blocking; without the
 * flag a sock_accept or sock_recv on an idle socket would stall the loop. */
int perry_wasi_socket_nonblock(int fd) {
    __wasi_errno_t e = __wasi_fd_fdstat_set_flags((__wasi_fd_t)fd, __WASI_FDFLAGS_NONBLOCK);
    return e ? -(int)e : 0;
}

int perry_wasi_tcp_open(const char *host, unsigned short port, int listening,
                        int backlog, int reuse_port, int nodelay, int *pending) {
    (void)host; (void)port; (void)backlog; (void)reuse_port; (void)nodelay;
    if (!listening) return -ENOTSUP;
    if (listener_count < 0) discover_listeners();
    if (listener_next >= listener_count) return -EADDRNOTAVAIL;
    int fd = listener_fds[listener_next++];
    int nb = perry_wasi_socket_nonblock(fd);
    if (nb < 0) return nb;
    *pending = 0;
    return fd;
}

int perry_wasi_socket_connect_ready(int fd) { (void)fd; return -ENOTSUP; }

/* The flags apply to the accepted socket, not to the listener. */
int perry_wasi_socket_accept(int fd, int nodelay) {
    (void)nodelay;
    __wasi_fd_t accepted;
    __wasi_errno_t e = __wasi_sock_accept((__wasi_fd_t)fd, __WASI_FDFLAGS_NONBLOCK, &accepted);
    return e ? -(int)e : (int)accepted;
}

int perry_wasi_socket_read(int fd, unsigned char *bytes, unsigned len) {
    __wasi_iovec_t iov = {bytes, len};
    __wasi_size_t n;
    __wasi_roflags_t flags;
    __wasi_errno_t e = __wasi_sock_recv((__wasi_fd_t)fd, &iov, 1, 0, &n, &flags);
    return e ? -(int)e : (int)n;
}

int perry_wasi_socket_write(int fd, const unsigned char *bytes, unsigned len) {
    __wasi_ciovec_t iov = {bytes, len};
    __wasi_size_t n;
    __wasi_errno_t e = __wasi_sock_send((__wasi_fd_t)fd, &iov, 1, 0, &n);
    return e ? -(int)e : (int)n;
}

int perry_wasi_socket_shutdown(int fd) {
    __wasi_errno_t e = __wasi_sock_shutdown((__wasi_fd_t)fd, __WASI_SDFLAGS_WR);
    return e ? -(int)e : 0;
}

int perry_wasi_socket_close(int fd) {
    /* The host reuses descriptor numbers: a closed listener must not lend its
     * announced address to whatever is accepted or opened at the same number. */
    for (int i = 0; i < listener_count; i++) {
        if (listener_fds[i] != fd) continue;
        listener_fds[i] = -1;
        listener_addrs[i][0] = 0;
    }
    __wasi_errno_t e = __wasi_fd_close((__wasi_fd_t)fd);
    return e ? -(int)e : 0;
}

/* preview 1 has no getsockname or getpeername: only a listener whose address
 * the embedder announced in PERRY_LISTEN_ADDRS has one; the caller falls back
 * to the address it asked for. */
int perry_wasi_socket_address(int fd, int peer, char *text, unsigned cap, unsigned short *port) {
    if (peer || listener_count <= 0) return -ENOTSUP;
    for (int i = 0; i < listener_count; i++) {
        if (listener_fds[i] != fd || !listener_addrs[i][0]) continue;
        const char *addr = listener_addrs[i];
        const char *colon = strrchr(addr, ':');
        if (!colon) return -EINVAL;
        size_t len = (size_t)(colon - addr);
        if (len >= 2 && addr[0] == '[' && addr[len - 1] == ']') { addr++; len -= 2; }
        if (len + 1 > cap) return -EINVAL;
        memcpy(text, addr, len);
        text[len] = 0;
        *port = (unsigned short)atoi(colon + 1);
        return 0;
    }
    return -ENOTSUP;
}

/* Block until one of `fds` is ready (kinds bit 0 read, bit 1 write) or
 * `timeout_ns` elapses. Fills `ready` with the same bits for each fd and
 * returns how many fds were ready, or a negative errno. One `poll_oneoff`
 * carries every subscription plus a relative monotonic clock, so an idle guest
 * sleeps in the host instead of spinning. */
int perry_wasi_poll(const int *fds, const unsigned char *kinds, unsigned n,
                    unsigned long long timeout_ns, unsigned char *ready) {
    size_t cap = 1 + 2 * (size_t)n;
    __wasi_subscription_t *subs = calloc(cap, sizeof(*subs));
    __wasi_event_t *events = calloc(cap, sizeof(*events));
    if (!subs || !events) { free(subs); free(events); return -ENOMEM; }
    size_t count = 0;
    subs[count].userdata = 0;
    subs[count].u.tag = __WASI_EVENTTYPE_CLOCK;
    subs[count].u.u.clock.id = __WASI_CLOCKID_MONOTONIC;
    subs[count].u.u.clock.timeout = timeout_ns;
    subs[count].u.u.clock.precision = 1000000;
    subs[count].u.u.clock.flags = 0;
    count++;
    for (unsigned i = 0; i < n; i++) {
        ready[i] = 0;
        if (kinds[i] & 1) {
            subs[count].userdata = i + 1;
            subs[count].u.tag = __WASI_EVENTTYPE_FD_READ;
            subs[count].u.u.fd_read.file_descriptor = (__wasi_fd_t)fds[i];
            count++;
        }
        if (kinds[i] & 2) {
            subs[count].userdata = i + 1;
            subs[count].u.tag = __WASI_EVENTTYPE_FD_WRITE;
            subs[count].u.u.fd_write.file_descriptor = (__wasi_fd_t)fds[i];
            count++;
        }
    }
    __wasi_size_t done = 0;
    __wasi_errno_t e = __wasi_poll_oneoff(subs, events, (__wasi_size_t)count, &done);
    int result = e ? -(int)e : 0;
    for (__wasi_size_t i = 0; !e && i < done; i++) {
        if (events[i].type == __WASI_EVENTTYPE_CLOCK) continue;
        unsigned at = (unsigned)events[i].userdata - 1;
        if (at >= n) continue;
        if (!ready[at]) result++;
        ready[at] |= events[i].type == __WASI_EVENTTYPE_FD_READ ? 1 : 2;
    }
    free(subs);
    free(events);
    return result;
}
#else
#include <netdb.h>
#include <netinet/in.h>
#include <string.h>
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/tcp.h>
#include <poll.h>
#include <stdio.h>
#include <sys/socket.h>
#include <unistd.h>

int perry_wasi_resolve(const char *hostname, int family, void *ctx,
                      void (*push)(void *, int, const unsigned char *)) {
    struct addrinfo hints = {0}, *results = 0;
    hints.ai_family = family == 4 ? AF_INET : family == 6 ? AF_INET6 : AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    int error = getaddrinfo(hostname, 0, &hints, &results);
    if (error) return error;
    for (struct addrinfo *entry = results; entry; entry = entry->ai_next) {
        if (entry->ai_family == AF_INET)
            push(ctx, 4, (const unsigned char *)&((struct sockaddr_in *)entry->ai_addr)->sin_addr);
        else if (entry->ai_family == AF_INET6)
            push(ctx, 6, (const unsigned char *)&((struct sockaddr_in6 *)entry->ai_addr)->sin6_addr);
    }
    freeaddrinfo(results);
    return 0;
}

/* Keep libc's sockaddr and poll layouts private to this translation unit.
 * Negative results are WASI errno values; no JS memory crosses this boundary. */
int perry_wasi_socket_nonblock(int fd) {
    int flags = fcntl(fd, F_GETFL, 0);
    if (flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0) return -errno;
    return 0;
}

int perry_wasi_tcp_open(const char *host, unsigned short port, int listening,
                        int backlog, int reuse_port, int nodelay, int *pending) {
    struct addrinfo hints = {0}, *addresses = 0;
    char service[6];
    snprintf(service, sizeof(service), "%u", (unsigned)port);
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_flags = listening ? AI_NUMERICHOST : 0;
    if (getaddrinfo(host, service, &hints, &addresses)) return -EHOSTUNREACH;
    int result = -EAFNOSUPPORT;
    for (struct addrinfo *a = addresses; a; a = a->ai_next) {
        int fd = socket(a->ai_family, SOCK_STREAM, 0);
        if (fd < 0) { result = -errno; continue; }
        int one = 1;
        if (listening) setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
        if (reuse_port) {
#ifdef SO_REUSEPORT
            if (setsockopt(fd, SOL_SOCKET, SO_REUSEPORT, &one, sizeof(one)) < 0) {
                result = -errno; close(fd); continue;
            }
#else
            result = -ENOTSUP; close(fd); continue;
#endif
        }
        if (nodelay) setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
        int nb = perry_wasi_socket_nonblock(fd);
        if (nb < 0) { result = nb; close(fd); continue; }
        if (listening) {
            if (bind(fd, a->ai_addr, a->ai_addrlen) < 0 || listen(fd, backlog) < 0) {
                result = -errno; close(fd); continue;
            }
            *pending = 0;
        } else if (connect(fd, a->ai_addr, a->ai_addrlen) < 0) {
            if (errno != EINPROGRESS && errno != EALREADY && errno != EAGAIN) {
                result = -errno; close(fd); continue;
            }
            *pending = 1;
        } else *pending = 0;
        result = fd;
        break;
    }
    freeaddrinfo(addresses);
    return result;
}

int perry_wasi_socket_connect_ready(int fd) {
    struct pollfd p = {fd, POLLOUT, 0};
    if (poll(&p, 1, 0) < 0) return -errno;
    if (!p.revents) return 0;
    int error = 0;
    socklen_t len = sizeof(error);
    if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &len) < 0) return -errno;
    return error ? -error : 1;
}

int perry_wasi_socket_accept(int fd, int nodelay) {
    int accepted = accept(fd, 0, 0);
    if (accepted < 0) return -errno;
    int error = perry_wasi_socket_nonblock(accepted);
    if (error < 0) { close(accepted); return error; }
    if (nodelay) { int one = 1; setsockopt(accepted, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one)); }
    return accepted;
}

int perry_wasi_socket_read(int fd, unsigned char *bytes, unsigned len) {
    ssize_t n = recv(fd, bytes, len, 0);
    return n < 0 ? -errno : (int)n;
}

int perry_wasi_socket_write(int fd, const unsigned char *bytes, unsigned len) {
    ssize_t n = send(fd, bytes, len, 0);
    return n < 0 ? -errno : (int)n;
}

int perry_wasi_socket_shutdown(int fd) { return shutdown(fd, SHUT_WR) < 0 ? -errno : 0; }
int perry_wasi_socket_close(int fd) { return close(fd) < 0 ? -errno : 0; }

int perry_wasi_socket_address(int fd, int peer, char *text, unsigned cap, unsigned short *port) {
    struct sockaddr_storage storage;
    socklen_t len = sizeof(storage);
    if ((peer ? getpeername(fd, (struct sockaddr *)&storage, &len)
              : getsockname(fd, (struct sockaddr *)&storage, &len)) < 0) return -errno;
    const void *ip;
    if (storage.ss_family == AF_INET) {
        struct sockaddr_in *a = (struct sockaddr_in *)&storage;
        ip = &a->sin_addr; *port = ntohs(a->sin_port);
    } else if (storage.ss_family == AF_INET6) {
        struct sockaddr_in6 *a = (struct sockaddr_in6 *)&storage;
        ip = &a->sin6_addr; *port = ntohs(a->sin6_port);
    } else return -EAFNOSUPPORT;
    return inet_ntop(storage.ss_family, ip, text, cap) ? 0 : -errno;
}
#endif
