(component
  (import "wasi:io/poll@0.2.0" (instance $poll
    (export "pollable" (type $pollable (sub resource)))
    (export "[method]pollable.block" (func (param "self" (borrow $pollable))))))
  (alias export $poll "pollable" (type $pollable))
  (import "wasi:clocks/monotonic-clock@0.2.0" (instance $clock
    (alias outer 1 $pollable (type $outer-pollable))
    (export "pollable" (type (eq $outer-pollable)))
    (export "subscribe-duration" (func (param "when" u64) (result (own $outer-pollable))))))
  (alias export $poll "[method]pollable.block" (func $block))
  (alias export $clock "subscribe-duration" (func $subscribe))
  (core func $block (canon lower (func $block)))
  (core func $subscribe (canon lower (func $subscribe)))
  (core func $drop (canon resource.drop $pollable))
  (core module $code
    (import "host" "subscribe" (func $subscribe (param i64) (result i32)))
    (import "host" "block" (func $block (param i32)))
    (import "host" "drop" (func $drop (param i32)))
    (func (export "wait") (param $duration i64) (local $pollable i32)
      (local.set $pollable (call $subscribe (local.get $duration)))
      (call $block (local.get $pollable))
      (call $drop (local.get $pollable))))
  (core instance $host
    (export "subscribe" (func $subscribe))
    (export "block" (func $block))
    (export "drop" (func $drop)))
  (core instance $code (instantiate $code (with "host" (instance $host))))
  (alias core export $code "wait" (core func $wait))
  (func (export "wait") (param "duration" u64) (canon lift (core func $wait))))
