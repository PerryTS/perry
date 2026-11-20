### Performance

The static loop unroller no longer clones a loop body that contains a function
literal (a closure, an arrow, an object-literal method or a class expression).
Each copy used to become its own compiled function, so objects built in a short
counted loop such as `for (let i = 0; i < 8; i++) objs.push({ m() { ... } })`
carried eight different code pointers for one source method, and a method call
site over them primed past its ways and went megamorphic. One source function
literal is now one function, and the site keeps a single entry.
