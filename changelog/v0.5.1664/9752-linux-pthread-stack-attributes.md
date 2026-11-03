### Fixed
- Use libc's typed pthread attributes for Linux GC and error-stack bounds, fixing conflicting declarations in the warnings gate and replacing manually sized attribute buffers with correctly aligned storage.
