On macOS, strip-dedup no longer localizes the shared exports of a kept bundled
member that defines writable or thread-local data. Splitting such a member
between the wrapper's copy and the stdlib's left `http.createServer().listen()`
failing with `listen EAGAIN` in every program, because perry-ffi's sink
registration flag was set in one copy and read from the other.
