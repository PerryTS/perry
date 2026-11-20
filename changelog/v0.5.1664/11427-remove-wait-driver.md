Remove the unregistered Tokio-era wait-driver and native-inflight hooks from
the runtime event pump (#11403), including their atomic callback slots,
transitional park/fast-drive branches, tick counters and unused drive metrics.
Turnloop and condvar waits remain, as does the native-submission wake API still
used by the FFI async bridge.

Update the loop-stats probes and server report to consume the remaining
metrics. The native probe now requires live turnloop turns and completions.
Remove tests that installed the deleted driver, retain the notify-before-park
check, and assert that formatted wait stats contain no obsolete counters.

Validation: 32 baseline event-pump tests passed; all 30 remaining tests pass
after removing the two obsolete driver-specific tests. In matching AArch64
`perry-dev` default-feature test builds, `js_wait_for_event` shrinks from 410
to 357 disassembled instructions and `js_notify_main_thread` from 172 to 167.
These are static instruction counts, not throughput or executed-instruction
measurements. The removed registration/slot symbols are absent from the new
binary; `js_native_work_submitted` remains.
