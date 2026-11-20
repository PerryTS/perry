Removed `BUFFER_LIKE_ADDR_FILTER`, the 1,024-bit set filter sitting behind the
buffer registry's address window. It was measured saturated: on an ordinary
claude-code command (startup, two `Read` calls, a streamed reply) the population
is 3,232 cumulative admissions and 1,618 live against 1,024 bits, so every bit
ends the run set and the filter rejects nothing — while still costing three hash
rounds and up to three dependent loads on each of ~26.6M admitted probes. The
adoption note's premises came from a single 400-character reply (213
registrations, 0.207% true positives) and do not hold at ordinary command scale,
where 88.87% of admitted probes find a real registered buffer, i.e. work the
registry genuinely has to do and no filter can remove.

The `RegistryAddrWindow` min/max bound stays and continues to do all of the
rejecting (15.51% on the measured command), including the debug-build machine
check that re-derives every rejection from the authoritative tables. Removing a
negative accelerator cannot change an answer, only the time taken to reach it.

Six interleaved pairs, one binary and one environment variable: minimum command
CPU 1.22s -> 1.19s (-2.46%), medians 1.28s -> 1.20s, paired median -3.60%,
faster in five pairs and tied in the sixth; peak RSS maxima 664.5 -> 635.2 MiB.
