Fix x86-64 shared runtime linking by resolving the array index guard through
the global offset table, preserving the process-wide provider's exported flag.
