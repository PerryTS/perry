// #1312 — process.env.<UNSET> must be nullish (undefined), so `?? default` applies.
const v = process.env.DEFINITELY_UNSET_VAR_XYZ;
console.log('typeof:', typeof v); // expect: undefined
console.log('value:', JSON.stringify(v)); // expect: undefined (JSON.stringify(undefined) prints "undefined")
console.log('?? :', JSON.stringify(v ?? 'FALLBACK')); // expect: "FALLBACK"
console.log('|| :', JSON.stringify(v || 'FALLBACK')); // expect: "FALLBACK"

// A SET var should be returned verbatim (and ?? must NOT clobber it).
const set = process.env.PERRY_1312_SET ?? 'FALLBACK';
console.log('set ??:', set); // expect: hello (when PERRY_1312_SET=hello)

// A var SET to empty string is falsy but NOT nullish: ?? keeps "", || swaps.
const empty = process.env.PERRY_1312_EMPTY;
console.log('empty typeof:', typeof empty); // expect: string
console.log('empty ?? :', JSON.stringify(empty ?? 'FALLBACK')); // expect: "" (kept)
console.log('empty || :', JSON.stringify(empty || 'FALLBACK')); // expect: "FALLBACK"

// Dynamic access path (EnvGetDynamic) should behave identically.
const key = 'DEFINITELY_UNSET_VAR_XYZ';
const dyn = process.env[key];
console.log('dyn typeof:', typeof dyn); // expect: undefined
console.log('dyn ?? :', JSON.stringify(dyn ?? 'FALLBACK')); // expect: "FALLBACK"
