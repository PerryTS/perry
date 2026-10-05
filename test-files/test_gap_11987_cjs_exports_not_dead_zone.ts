// #11987: a CommonJS module's exports are properties of `module.exports`, not
// lexical bindings, so reading them after the module ran never throws. The
// #11826 cyclic-export check once seeded the CJS wrap's `export const X =
// _cjs.X` with the dead-zone sentinel; that declarator never stores X's
// global (its getter reads `module.exports` live), so the check right after
// it threw `Cannot access 'X' before initialization` in qs (arrayToObject) and
// tsc (ANONYMOUS).
import utils from "./_helpers/cjs_export_11987/utils.cjs";
import bundle from "./_helpers/cjs_export_11987/bundle.cjs";

console.log(JSON.stringify(utils.arrayToObject([5, 6])));
console.log(JSON.stringify(utils.merge([1], 2)));
console.log(utils.format);
console.log(bundle.describe(""), "|", bundle.describe("f"), "|", bundle.ANONYMOUS);
