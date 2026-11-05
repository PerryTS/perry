(component
  ;; Origin interface `shapes`, exporting `point`.
  (type (;0;)
    (instance
      (type (;0;) (record (field "x" s32)))
      (export (;1;) "point" (type (eq 0)))
    )
  )
  (import "test:lib/shapes" (instance (;0;) (type 0)))
  (alias export 0 "point" (type (;1;)))

  ;; A record whose field references `point` from `shapes` by id. wit-component
  ;; produces this shape for a world that imports `shapes` and `use`s a record
  ;; built on `point`.
  (type (;2;) (record (field "origin" 1)))
  (import "region" (type (;3;) (eq 2)))

  (core module (;0;)
    (type (;0;) (func (result i32)))
    (func (;0;) (type 0) (result i32) i32.const 7)
    (export "run" (func 0))
  )
  (core instance (;0;) (instantiate 0))
  (alias core export 0 "run" (core func (;0;)))
  (type (;4;) (func (result s32)))
  (func (;0;) (type 4) (canon lift (core func 0)))
  (export "run" (func 0))
)
