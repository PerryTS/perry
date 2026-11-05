let numberToArray: any = 0;
numberToArray = [numberToArray];
console.log(
  "number-to-array",
  Array.isArray(numberToArray),
  numberToArray instanceof Array,
  typeof numberToArray,
);

let stringToArray: any = "s";
stringToArray = [stringToArray];
console.log(
  "string-to-array",
  Array.isArray(stringToArray),
  stringToArray instanceof Array,
  typeof stringToArray,
);

let nullToArray: any = null;
nullToArray = [nullToArray];
console.log(
  "null-to-array",
  Array.isArray(nullToArray),
  nullToArray instanceof Array,
  typeof nullToArray,
);

let arrayToNumber: any = [9];
arrayToNumber = 42;
console.log(
  "array-to-number",
  Array.isArray(arrayToNumber),
  arrayToNumber instanceof Array,
  typeof arrayToNumber,
);

let arrayToString: any = [9];
arrayToString = "s";
console.log(
  "array-to-string",
  Array.isArray(arrayToString),
  arrayToString instanceof Array,
  typeof arrayToString,
);

const unchanged: any = [1];
console.log(
  "unchanged-array",
  Array.isArray(unchanged),
  unchanged instanceof Array,
  typeof unchanged,
);
