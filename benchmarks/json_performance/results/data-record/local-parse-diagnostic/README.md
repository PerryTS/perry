Local diagnostic for remaining parse regressions; not a quiet CPU/RSS run.

Three fresh processes per fixture/build, fixed work, shuffled engine order,
default GC. Process-wide instruction counts include startup and warmup.
All 27 executions completed successfully. Separate PERRY_GC_DIAG=1 executions
also completed; their timings do not count toward acceptance.

Compared with Unicode, the record worker retires 0.23% more instructions for
numeric parsing, versus the qualified shared-M1 CPU regression of 8.03%.
All three instrumented numeric runs (Unicode/tape/record, 128 calls and two
warmups) report nine old_reclaim_alloc_point scans, 8 MiB final arena capacity,
and approximately 3.35 MB final live arena bytes. These observations do not
support attributing the whole slowdown to more parser instructions or a larger
number of those GC scans. They do not prove equal GC time or exclude GC costs.
The next numeric investigation should inspect execution efficiency and the
sampled hot loops, rather than assume an additional allocation or input copy.

Tiny-object parsing retires 2.33% more instructions than tape, consistent with
investigating the scalar eligibility/wrapper overhead on the container path.
Heterogeneous parsing retires 1.50% more instructions than Unicode.
Raw cycle counts vary on this busy development host and are not speed claims.
