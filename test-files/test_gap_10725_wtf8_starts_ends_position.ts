// #10725: position-based startsWith/endsWith must count WTF-8 lone
// surrogates as one UTF-16 code unit without treating their bytes as UTF-8.
// Build both surrogates at runtime so the receiver reaches the runtime path.
const high = String.fromCharCode(0xd800);
const low = String.fromCharCode(0xdc00);
const source = high + "abc" + low + "xyz";

console.log("starts after high", source.startsWith("abc", 1));
console.log("starts before high", source.startsWith("abc", 0));
console.log("starts at high", source.startsWith(high, 0));
console.log("starts at low", source.startsWith(low, 4));
console.log("starts after low", source.startsWith("xyz", 5));
console.log("starts before low", source.startsWith("xyz", 4));

console.log("ends after high", source.endsWith(high, 1));
console.log("ends before high", source.endsWith(high, 0));
console.log("ends before low", source.endsWith("abc", 4));
console.log("ends inside abc", source.endsWith("abc", 3));
console.log("ends after low", source.endsWith(low, 5));
console.log("ends after xyz", source.endsWith("xyz", 8));
