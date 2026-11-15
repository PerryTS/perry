A method call that finds no such method on an Array, a Date or a typed array (`p.m()` with `p: P` annotated as a
class but given `[7, 8]`) throws node's TypeError `p.m is not a function`; it used to answer an empty object.
