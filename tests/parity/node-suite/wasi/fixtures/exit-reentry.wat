(module
  (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
  (import "env" "callback" (func $callback))
  (memory (export "memory") 1)
  (func (export "abort") i32.const 9 call $exit)
  (func (export "_start") call $callback i32.const 0 i32.const 99 i32.store)
)
