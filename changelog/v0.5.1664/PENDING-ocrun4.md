Fix inherited symbol reads through fresh class constructors. A factory-created
class keeps its evaluated constructor parent; another class extending it can
inherit an iterator from a function-valued ancestor. OpenCode's plugin Effect
generator previously tried to call `next` on RuntimeFlags.Service itself.

Symbol lookup now follows the constructor evaluation's own parent in the
ordinary prototype walk, and a declared class whose heritage is a fresh class
evaluation continues the read on that exact parent (the edge
`Object.getPrototypeOf` already reports). This removes the separate closure-only walk and its
dependence on shared template heritage; no cache, latch or side table is added.
The regression was exposed by #12191's 66b7275c follow-up, which correctly stopped
publishing fresh evaluation heritage into the shared template.

The regression test creates two factory evaluations and checks delegated
iteration and the distinct inherited labels against Node.
