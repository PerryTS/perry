// #4511 — legacy escape()/unescape() (ES Annex B) + Node `global` alias

// Bare calls
console.log(escape("a b+c"), unescape("a%20b"));
// Non-ASCII: code units < 256 -> %XX, BMP -> %uXXXX, astral -> surrogate pair
console.log(escape("café ☃ 𝟘"));
console.log(unescape("caf%E9 %u2603"));

// typeof — both are functions
console.log(typeof escape, typeof unescape);
console.log(typeof globalThis.escape, typeof globalThis.unescape);

// Rebound as values
const e = escape;
const u = unescape;
console.log(e("100%"), u("%41%42"));

// Through globalThis explicitly
console.log(globalThis.escape("@*_+-./"), globalThis.unescape("%7E"));

// Malformed / partial escapes pass through unchanged
console.log(unescape("%"), unescape("%2"), unescape("%zz"), unescape("100%"));

// Node `global` object: alias for globalThis, typeof "object"
console.log(typeof global, global === globalThis);
(global as any).__perry_4511 = 7;
console.log((global as any).__perry_4511, (globalThis as any).__perry_4511);
