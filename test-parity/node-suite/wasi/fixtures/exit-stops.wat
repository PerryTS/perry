(module
  (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
  (memory (export "memory") 1)
  (func (export "_start")
    i32.const 7 call $exit
    i32.const 0 i32.const 99 i32.store)
)
