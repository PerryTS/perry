(component
  (type (export "f") (func))
  (import "foo" (func))
  (import "i1" (instance))
  (import "merged" (instance
      (export "a" (func))
      (export "b" (func))
  ))
  (export "bar" (func 0))
)
