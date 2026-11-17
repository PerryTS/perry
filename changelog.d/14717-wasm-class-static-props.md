On the web target a class used as a value holds static-side properties:
`(T as any).key = "class"` is kept and read back, and `Object.setPrototypeOf(T, p)`
makes the class inherit from `p` instead of doing nothing.
