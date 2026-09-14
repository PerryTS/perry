Fix dynamic HTTP, HTTPS, and HTTP/2 `server.listen()` argument handling. The handle dispatcher passed a stack buffer shaped like an array to the managed-array accessor, which returned `NaN` for both the requested port and completion callback. This could bind the default port and leave the caller waiting for a callback that was lost (#10137).

Share listen-overload parsing between real runtime arrays and borrowed argument values, then pass the parsed arguments directly to the existing server implementations. Remove the same fabricated-array pattern from `Bun.serve`. Real arrays retain the offset-aware accessor, including arrays whose dense queue prefix has been shifted away; no runtime/codegen array layout changes are needed.

Tests cover borrowed port/host/backlog/callback overloads and a real shifted argument array. The original HTTP/2 settings/ping/close callback fixture, which timed out before the fix, now matches Node.
