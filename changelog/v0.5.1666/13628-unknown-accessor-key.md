wpfacts: an accessor whose getter or setter is an unknown function value now
defines only the key it is installed under. It used to count as a computed
store into ⊤, which marked every key of every builtin prototype written, so
every key a program object gains late read as an opaque member (2,536 such
writes per unit in tssrc).
