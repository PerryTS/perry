Fix CommonJS cycle re-entry after `module.exports` is replaced, including
esbuild's `__export` / `__toCommonJS` getter exports and semver's exported
Comparator class. Partial publication now retains the module record, so the
existing require adapter reads its current exports instead of the initial
empty object. This adds no getter enumeration, copies, allocations, or extra
publication calls to ordinary CommonJS initialization.

Generated circular-dependency warnings check whether the property actually
exists before warning, without invoking accessors. Regression coverage imports
two esbuild-style CommonJS modules from ESM, checks the module views during
cycle re-entry, calls through the cycle, and verifies live bindings and empty
stderr. Related: #10178, #10107.
