(component
  (type (;0;)
    (instance
      (type (;0;) (func))
      (export (;0;) "do-something" (func (type 0)))
    )
  )
  (import "test:test/iface" (instance (;0;) (type 0)))
  (export "primary" (implements "test:test/iface") (instance 0))
  (export "backup" (implements "test:test/iface") (instance 0))
)
