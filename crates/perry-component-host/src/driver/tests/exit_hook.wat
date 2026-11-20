;; Prototype an exit notification without changing the original task-local
;; context, nominal component types, or task.return calling convention.
;; The callback ABI's low four return bits describe EXIT/YIELD/WAIT.
  (core module $tracked-code
    (import "original" "run" (func $run (param i64) (result i32)))
    (import "original" "callback" (func $callback (param i32 i32 i32) (result i32)))
    (import "original" "exited" (func $exited))
    (func (export "run") (param i64) (result i32) (local $status i32)
      (local.set $status (call $run (local.get 0)))
      (if (i32.eqz (i32.and (local.get $status) (i32.const 15))) (then (call $exited)))
      (local.get $status))
    (func (export "callback") (param i32 i32 i32) (result i32) (local $status i32)
      (local.set $status (call $callback (local.get 0) (local.get 1) (local.get 2)))
      (if (i32.eqz (i32.and (local.get $status) (i32.const 15))) (then (call $exited)))
      (local.get $status)))
  (core instance $tracked (instantiate $tracked-code (with "original" (instance
    (export "run" (func $code "run"))
    (export "callback" (func $code "callback"))
    (export "exited" (func $exited))))))
