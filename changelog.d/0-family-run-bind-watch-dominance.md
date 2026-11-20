- **`this.f = this.f.bind(this)` beside a user `bind` method.** A constructor
  store of a bound method that continues its run of family stores failed LLVM
  verification ("Instruction does not dominate all uses!") when some class in
  the program had its own `bind` method (three.js's WebGPU Renderer next to
  PropertyBinding). The `.bind` call then lowers as a dispatch over the
  program's `bind` methods; the store's run proof used the generic arm's
  "builtin bind ran" value after the dispatch merged. The bind watch is now
  held to the same dominance rule as the run it extends.
