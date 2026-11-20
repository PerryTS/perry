Reduce RegExp exec result work when a nonescaping result is used only for null checks and one numbered capture. Preserve method lookup order and runtime builtin/brand checks, and poll inline capture output by bounded work instead of once per tiny string.

Also make builtin RegExp exec reject Symbol arguments and Symbol-valued primitive coercions, as required by JavaScript ToString.
