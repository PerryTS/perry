codegen: the canonical-i32 profitability collector (`collectors/repsel_benefit.rs`,
#7128) is deleted. Whether a local is worth an i32 slot is the whole-program
facts' int demand (representation RFC §10.3): a local the facts cover takes the
slot exactly when they give it `I32`, so the per-body refusal ("written after
its declaration, no i32-consuming read, a double-consuming read in a loop")
only ever decided for a local the facts do not cover. On the chess engine, the
three.js skin bench and `benchmarks/suite` it refused nothing (binaries
byte-identical, `15_mandelbrot` included). A local the facts do not cover keeps
the collectors' value proof. The dispatch-tower routing threshold (#7142) in
the same file stays.
