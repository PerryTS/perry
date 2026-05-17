// Issue #923 fixture — block-export form `const x = ...; export { x };`
// Pre-fix this lowered to `Export::Named { local: "pool", exported: "pool" }`
// for a non-function local, which the wrapper-emission loops in codegen
// skipped, leaving `__perry_wrap_perry_fn_<src>__pool` undefined and the
// link failing when consumers passed `pool` as a value.

const pool = { name: "shared-pool", tag: 42 };

export function poolName(): string {
  return pool.name;
}

export { pool };
