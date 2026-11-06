(module
  (memory (export "memory") 1)
  (func (export "_start")
    i32.const 0
    i32.const 42
    i32.store8))
