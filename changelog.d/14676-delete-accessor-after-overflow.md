`delete C.prototype["g"]` of a class-body getter or setter (or of an object-literal getter) on an object that already
holds keys past its layout now makes `"g" in C.prototype`, `hasOwnProperty` and `Object.hasOwn` false, as in node;
the presence probe answered from the layout's accessor entry and kept saying true.
