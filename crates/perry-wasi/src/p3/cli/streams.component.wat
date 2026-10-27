;; Transfer stdin's stream through guest memory to stdout, returning both
;; completion futures to the host. No stream or future is substituted in Rust.
(component
  (import "wasi:cli/types@0.3.0" (instance $types
    (type $error (enum "io" "illegal-byte-sequence" "pipe"))
    (export "error-code" (type (eq $error)))))
  (alias export $types "error-code" (type $error))
  (type $outcome (result (error $error)))
  (type $completion (future $outcome))
  (type $bytes (stream u8))
  (import "wasi:cli/stdin@0.3.0" (instance $stdin
    (alias outer 1 $error (type $error))
    (alias outer 1 $completion (type $completion))
    (alias outer 1 $bytes (type $bytes))
    (export "error-code" (type (eq $error)))
    (export "read-via-stream" (func (result (tuple $bytes $completion))))))
  (import "wasi:cli/stdout@0.3.0" (instance $stdout
    (alias outer 1 $error (type $error))
    (alias outer 1 $completion (type $completion))
    (alias outer 1 $bytes (type $bytes))
    (export "error-code" (type (eq $error)))
    (export "write-via-stream" (func (param "data" $bytes) (result $completion)))))
  (alias export $stdin "read-via-stream" (func $read))
  (alias export $stdout "write-via-stream" (func $write))
  (core module $memory (memory (export "memory") 1))
  (core instance $memory (instantiate $memory))
  (core func $read (canon lower (func $read) (memory (core memory $memory "memory"))))
  (core func $write (canon lower (func $write)))
  (core module $guest
    (import "memory" "memory" (memory 1))
    (import "host" "read" (func $read (param i32)))
    (import "host" "write" (func $write (param i32) (result i32)))
    (func (export "run") (result i32)
      (call $read (i32.const 16))
      (i32.store (i32.const 0) (i32.load (i32.const 20)))
      (i32.store (i32.const 4) (call $write (i32.load (i32.const 16))))
      (i32.const 0)))
  (core instance $guest (instantiate $guest
    (with "memory" (instance $memory))
    (with "host" (instance (export "read" (func $read)) (export "write" (func $write))))))
  (func (export "run") (result (tuple $completion $completion))
    (canon lift (core func $guest "run") (memory (core memory $memory "memory")))))
