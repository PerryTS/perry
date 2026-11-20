On the web target a closure used as a value is a function: `typeof` gives
`"function"`, and calling it through an object property, a computed key or a
local runs it, async bodies included.
