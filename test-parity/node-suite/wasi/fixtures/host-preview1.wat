(module
  (import "wasi_snapshot_preview1" "fd_prestat_get" (func $prestat (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open" (func $open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_read" (func $read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_close" (func $close (param i32) (result i32)))
  (import "wasi_snapshot_preview1" "random_get" (func $random (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 128) "data.txt")
  (data (i32.const 160) "../outside.txt")
  (data (i32.const 192) "escape")
  (func (export "_initialize"))
  (func (export "prestat") (result i32) i32.const 3 i32.const 0 call $prestat)
  (func (export "open") (param $path i32) (param $len i32) (result i32)
    i32.const 3 i32.const 1 local.get $path local.get $len i32.const 0
    i64.const 2 i64.const 0 i32.const 0 i32.const 64 call $open)
  (func (export "read") (param $fd i32) (result i32)
    i32.const 0 i32.const 512 i32.store
    i32.const 4 i32.const 128 i32.store
    local.get $fd i32.const 0 i32.const 1 i32.const 8 call $read)
  (func (export "write") (result i32)
    i32.const 0 i32.const 128 i32.store
    i32.const 4 i32.const 8 i32.store
    i32.const 1 i32.const 0 i32.const 1 i32.const 8 call $write)
  (func (export "close") (param i32) (result i32) local.get 0 call $close)
  (func (export "grow_random") (result i32)
    i32.const 1 memory.grow drop
    i32.const 65536 i32.const 32 call $random)
  (func (export "bad_random") (result i32)
    i32.const -1 i32.const 32 call $random)
  (func (export "exit") (param i32)
    local.get 0 call $exit
    i32.const 100 i32.const 99 i32.store)
)
