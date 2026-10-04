Diagnostic report names no longer cost load-time relocations. The IC-miss,
method-site, receiver-repr and store-census reports kept their names in
`[&str; N]` tables; every entry is an absolute pointer that the dynamic loader
patches at startup in every program that links the path, armed or not. They
are now one string each, split when a report prints, and the method-site
report arms through a byte instead of a `OnceLock`. This removes the
hello-world startup cost #11894 added (the promise `then` read site links the
IC miss path into programs with no property reads): hello world is now below
its pre-#11894 instruction count.
