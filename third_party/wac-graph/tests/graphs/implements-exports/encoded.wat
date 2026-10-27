(component
  (type (;0;)
    (instance
      (type (;0;) (func))
      (export (;0;) "do-something" (func (type 0)))
    )
  )
  (import "test:test/iface" (instance (;0;) (type 0)))
  (type (;1;)
    (component
      (type (;0;)
        (instance
          (type (;0;) (func))
          (export (;0;) "do-something" (func (type 0)))
        )
      )
      (import "test:test/iface" (instance (;0;) (type 0)))
      (export (;1;) "primary" (implements "test:test/iface") (instance (type 0)))
      (export (;2;) "backup" (implements "test:test/iface") (instance (type 0)))
    )
  )
  (import "unlocked-dep=<test:producer>" (component (;0;) (type 1)))
  (instance (;1;) (instantiate 0
      (with "test:test/iface" (instance 0))
    )
  )
)
