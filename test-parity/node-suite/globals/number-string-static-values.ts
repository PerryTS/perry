function showStatic(label: string, fn: any) {
  const nameDesc = Object.getOwnPropertyDescriptor(fn, "name");
  const lengthDesc = Object.getOwnPropertyDescriptor(fn, "length");
  console.log(label, "typeof:", typeof fn);
  console.log(label, "name/length:", fn?.name, fn?.length);
  console.log(label, "name desc:", JSON.stringify(nameDesc));
  console.log(label, "length desc:", JSON.stringify(lengthDesc));
}

const numberIsInteger = Number.isInteger;
const numberIsSafeInteger = Number.isSafeInteger;
const stringFromCharCode = String.fromCharCode;
const stringFromCodePoint = String.fromCodePoint;
const stringRaw = String.raw;

showStatic("Number.isInteger", numberIsInteger);
showStatic("Number.isSafeInteger", numberIsSafeInteger);
showStatic("String.fromCharCode", stringFromCharCode);
showStatic("String.fromCodePoint", stringFromCodePoint);
showStatic("String.raw", stringRaw);

console.log(
  "Direct calls:",
  Number.isInteger(4),
  String.fromCharCode(88, 89),
  String.raw({ raw: ["x", "y"] }, "-"),
);
console.log(
  "Number calls:",
  numberIsInteger(3),
  numberIsInteger(3.5),
  numberIsSafeInteger(9007199254740991),
  numberIsSafeInteger(9007199254740992),
);
console.log(
  "String calls:",
  stringFromCharCode(65, 66, 67),
  stringFromCodePoint(65, 128512),
  stringRaw({ raw: ["a", "b", "c"] }, 1, 2),
);
console.log("Number.parseInt identity:", Number.parseInt === parseInt);
console.log("Number.isNaN meta:", Number.isNaN.name, Number.isNaN.length);
