Keep whole-program CommonJS publications separate by canonical module path, with conservative unknown-path and dynamic lookup flows. Treat the registry union as an analysis relation, and model the shared main bootstrap record separately from module exports.

Collect Object.defineProperties descriptor values from own properties and getters, including getters discovered later by the solver. This prevents inherited constructor values from creating false descriptor/accessor flows while retaining genuine loader and interpreter exposure.
