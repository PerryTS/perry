/* wasi-sdk's libc implements WASIp2 name lookup. Rust std's resolver on
 * this target is still unsupported; keep libc layouts on the C side. */
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
