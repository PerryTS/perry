`class X extends LRUCache` compiles, including constructor-less subclasses.

`lru-cache` is one of the packages perry serves from a bundled native binding
rather than compiling from npm source, and that binding is a compile-time
lowering with no runtime class value. Heritage naming it reached the dynamic
parent-registration path and threw `Class extends value is not a constructor`
before any user code ran.

`LRUCache` is now recognised as a native parent and routed to the same
subclass-init pattern `EventEmitter` and the stream classes already use, so the
instance carries the native surface whether or not the subclass declares its own
constructor. `has`/`delete` on such an instance answer booleans rather than the
raw handle value.
