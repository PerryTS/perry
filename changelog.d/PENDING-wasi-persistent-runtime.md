Connect standalone component hosts to the persistent native WASI driver. Each
queued invocation owns traced arguments, async context, Promise and first-error
state; callbacks rebuild fresh origin-specific frames on every poll. Calls can
overlap and settle independently, while synchronous exports retain plain return
values. Initialization receives an initial poll even without an export call.

Stage every completed native reply before JavaScript settlement hooks run, so
nested disposal preserves ready replies and reentrant submissions survive older
retention snapshots. Keep import-result roots until validated ownership receipts
arrive, and retry collector requests after admission pressure subsides. Release
the native owner pin when a driver becomes idle.

Require matching component-host/runtime ABI 10. Incoming HTTP remains on its
existing path while its persistent handoff is integrated; future/stream wrappers,
async guest adapters and final platform/package qualification remain outstanding.

Validate 66 component and 12 async-hooks tests in development and unprivileged
production panic-abort, plus compiled production overlap/resource/command/HTTP
checks with moving GC, verified evacuation and protected from-space pages.
Record the actual before-fix rejection and collector-cancellation failures in
`docs/validation/wasi-persistent-runtime-20261026.json`.
