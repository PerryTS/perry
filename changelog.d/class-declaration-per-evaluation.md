Fixed a class declaration inside a function returning the same class on every
call. `function make(n) { class K { static s = n } return K }` gave
`make(1) === make(2)`, with the last call's statics, captures and prototype
shared by all of them; a sibling `class J extends K` also linked to that shared
class. Every evaluation now creates its own class object with its own statics
and prototype, as class expressions already did, and a function or class
method that names the class before its declaration reads that evaluation's
class. Declarations at module top level are unchanged.
