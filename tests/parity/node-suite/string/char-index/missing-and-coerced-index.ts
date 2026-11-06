// #2787: String character-access methods coerce a missing, `undefined`, or
// `NaN` index to 0 (JS ToIntegerOrInfinity); negative and out-of-range
// indices keep their per-method out-of-bounds shapes.
const s = "abc";

console.log("charAt:missing", JSON.stringify(s.charAt()));
console.log("charAt:undefined", JSON.stringify(s.charAt(undefined)));
console.log("charAt:NaN", JSON.stringify(s.charAt(NaN)));
console.log("charAt:-1", JSON.stringify(s.charAt(-1)));
console.log("charAt:99", JSON.stringify(s.charAt(99)));

console.log("charCodeAt:missing", s.charCodeAt());
console.log("charCodeAt:undefined", s.charCodeAt(undefined));
console.log("charCodeAt:NaN", s.charCodeAt(NaN));
console.log("charCodeAt:-1", s.charCodeAt(-1));
console.log("charCodeAt:99", s.charCodeAt(99));

console.log("codePointAt:missing", s.codePointAt());
console.log("codePointAt:undefined", s.codePointAt(undefined));
console.log("codePointAt:NaN", s.codePointAt(NaN));
console.log("codePointAt:-1", s.codePointAt(-1));
console.log("codePointAt:99", s.codePointAt(99));

console.log("at:missing", JSON.stringify(s.at()));
console.log("at:undefined", JSON.stringify(s.at(undefined)));
console.log("at:NaN", JSON.stringify(s.at(NaN)));
console.log("at:-1", JSON.stringify(s.at(-1)));
console.log("at:99", JSON.stringify(s.at(99)));
