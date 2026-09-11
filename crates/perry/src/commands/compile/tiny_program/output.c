/* The proven program's entire output is immutable and known at compile time.
 * Two cursors drain it without a heap, JS callbacks, or Perry's event loop.
 * POSIX pipe/socket console writes are asynchronous in Node: a blocked stdout
 * must not prevent a subsequent stderr write (e.g. a reader's handshake).
 * Files and terminals retain synchronous writes. Console ignores write errors.
 */
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stddef.h>
#include <sys/stat.h>
#include <unistd.h>

struct perry_tiny_output {
    int fd;
    const unsigned char *bytes;
    size_t length;
};

_Static_assert(sizeof(struct perry_tiny_output) == 24, "LLVM output record size");
_Static_assert(offsetof(struct perry_tiny_output, bytes) == 8, "LLVM output pointer offset");
_Static_assert(offsetof(struct perry_tiny_output, length) == 16, "LLVM output length offset");

struct stream_cursor {
    size_t record;
    size_t offset;
    int initialized;
    int closed;
    int pending;
    int asynchronous;
};

static void initialize_stream(int fd, struct stream_cursor *cursor) {
    struct stat info;
    cursor->initialized = 1;
    if (fstat(fd, &info) < 0) {
        cursor->closed = 1;
        return;
    }
    if (S_ISFIFO(info.st_mode) || S_ISSOCK(info.st_mode)) {
        cursor->asynchronous = 1;
        int flags = fcntl(fd, F_GETFL);
        if (flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0)
            cursor->closed = 1;
    } else if (isatty(fd)) {
        /* Node's POSIX terminal console is synchronous, even if the parent
         * supplied a nonblocking descriptor. */
        int flags = fcntl(fd, F_GETFL);
        if (flags < 0 || fcntl(fd, F_SETFL, flags & ~O_NONBLOCK) < 0)
            cursor->closed = 1;
    }
}

static void write_current(const struct perry_tiny_output *outputs,
                          struct stream_cursor *cursor) {
    const struct perry_tiny_output *output = &outputs[cursor->record];
    while (cursor->offset < output->length) {
        ssize_t written = write(output->fd, output->bytes + cursor->offset,
                                output->length - cursor->offset);
        if (written > 0) {
            cursor->offset += (size_t)written;
            continue;
        }
        if (written < 0 && errno == EINTR)
            continue;
        if (written < 0 && cursor->asynchronous &&
            (errno == EAGAIN || errno == EWOULDBLOCK)) {
            cursor->pending = 1;
            return;
        }
        cursor->closed = 1;
        break;
    }
    cursor->pending = 0;
}

void perry_tiny_run_output(const struct perry_tiny_output *outputs, size_t count) {
    struct stream_cursor streams[2] = {{0}};
    (void)signal(SIGPIPE, SIG_IGN);
    /* Preserve source call order for each initial write. If a stream fills,
     * its later calls stay queued in the immutable output table; the other
     * stream continues independently. No allocation or byte copying occurs. */
    for (size_t i = 0; i < count; ++i) {
        const int fd = outputs[i].fd; /* proof: exactly 1 or 2 */
        struct stream_cursor *cursor = &streams[fd - 1];
        if (!cursor->initialized)
            initialize_stream(fd, cursor);
        if (cursor->closed || cursor->pending)
            continue;
        cursor->record = i;
        cursor->offset = 0;
        write_current(outputs, cursor);
    }
    /* Only pending output can keep this finite program alive. poll() waits
     * for its two standard streams; it cannot run JS or create new work. */
    while (streams[0].pending || streams[1].pending) {
        struct pollfd fds[2] = {
            {streams[0].pending ? 1 : -1, POLLOUT, 0},
            {streams[1].pending ? 2 : -1, POLLOUT, 0},
        };
        int ready = poll(fds, 2, -1);
        if (ready < 0) {
            if (errno == EINTR)
                continue;
            return;
        }
        for (int stream = 0; stream < 2; ++stream) {
            struct stream_cursor *cursor = &streams[stream];
            if (!cursor->pending || !fds[stream].revents)
                continue;
            for (;;) {
                write_current(outputs, cursor);
                if (cursor->closed || cursor->pending)
                    break;
                size_t next = cursor->record + 1;
                while (next < count && outputs[next].fd != stream + 1)
                    ++next;
                if (next == count)
                    break;
                cursor->record = next;
                cursor->offset = 0;
            }
        }
    }
}
