Prove array iteration for the whole program from the wpfacts write analysis
(#13645). Two program facts join the builtin-prototype write record: no
compiled write can change which `@@iterator` an array finds (a builtin
prototype or unknown value written under `Symbol.iterator`, `__proto__` or any
key; an own iterator, prototype change or definition on a value that may be an
array, with a computed store decided by its solved key; an Array subclass with
an iterator member; `Reflect.construct` with a new target), and no compiled
write can give `%ArrayIteratorPrototype%` or its chain a `next` or `return`.
With both, `for...of` over an array skips the iteration-member read at entry
and emits nothing for the array arm of IteratorClose; the protocol arm is
unchanged. Interpreted code withholds both facts. The store standing for other
units' code no longer counts as a `Function.prototype.bind` write (those units
note their own stores), so per-unit scripts keep that fact.
