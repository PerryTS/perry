(component
  (type (;0;)
    (instance
      (type (;0;) (func))
      (export (;0;) "do-something" (func (type 0)))
    )
  )
  (import "primary" (implements "test:test/iface") (instance (;0;) (type 0)))
  (import "backup" (implements "test:test/iface") (instance (;1;) (type 0)))
  (type (;1;)
    (component
      (type (;0;)
        (instance
          (type (;0;) (func))
          (export (;0;) "do-something" (func (type 0)))
        )
      )
      (import "primary" (implements "test:test/iface") (instance (;0;) (type 0)))
      (import "backup" (implements "test:test/iface") (instance (;1;) (type 0)))
    )
  )
  (import "unlocked-dep=<test:consumer>" (component (;0;) (type 1)))
  (instance (;2;) (instantiate 0
      (with "primary" (instance 0))
      (with "backup" (instance 1))
    )
  )
)
