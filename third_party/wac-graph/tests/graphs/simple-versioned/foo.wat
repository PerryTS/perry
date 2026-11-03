(component
    (type 
        (instance 
            (type (record (field "x" u32)))
            (export "x" (type (eq 0)))
            (type (func (param "x" 1)))
            (export "y" (func (type 2)))
            (export "z" (type (sub resource)))
        )
    )
    (import "foo:bar/types@0.2.0" (instance (type 0)))
)