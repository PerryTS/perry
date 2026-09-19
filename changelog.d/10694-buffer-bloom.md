`is_registered_buffer` now consults a 1024-bit Bloom filter before the
thread-local hash sets. On a `tsc --noEmit` the probes that reached a hash
lookup fall from 21,646,032 to 862 — the registry never holds more than 9
buffers — while the answers are unchanged (#10694).
