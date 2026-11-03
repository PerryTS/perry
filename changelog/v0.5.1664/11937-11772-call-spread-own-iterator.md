### Call spread honors an Array's own iterator (#11772)

Function and method calls with spread arguments now run an Array source's own
`Symbol.iterator` instead of copying its indexed elements. The call-spread
materializer now shares the shape-aware dense-spread proof used by array
literals and spread `push`, retaining the fast path only when iteration is
unobservable.
