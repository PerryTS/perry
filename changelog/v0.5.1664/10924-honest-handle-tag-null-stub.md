The value perry hands back when an import or a method dispatch has nowhere to go
(the "unresolved-namespace stub") is now a real empty object. It used to be the
address of a `.rodata` byte array laid out like an object header but with no GC
header in front of it, so every type probe read whatever bytes the linker had
placed before it. In a v0.5.1631 build those bytes were the tail of a string
literal, and the stub reported itself as a heap kind that does not exist:
`JSON.stringify` of it answered `""` and `String()` of it threw `TypeError:
Cannot convert object to primitive value`. Both now answer as `{}` does
(`"{}"`, `"[object Object]"`), and the answers no longer depend on how the
binary happened to be linked (#10917).

One behaviour change follows from the stub now being the empty object it always
claimed to be: calling a method on it (`stub.raw()`) throws `TypeError: raw is
not a function`, exactly as it does on any `{}` and as node does. It used to
return the stub again, but only because the fake header routed the call into a
fallback arm; perry had already stopped doing that for real empty objects.
