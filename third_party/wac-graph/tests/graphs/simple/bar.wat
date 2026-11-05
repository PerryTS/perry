(component
  (import "e" (func))
  (import "i2" (instance))
  (import "merged" (instance
    (export "a" (func))
  ))
  (export "e" (func 0))
)
