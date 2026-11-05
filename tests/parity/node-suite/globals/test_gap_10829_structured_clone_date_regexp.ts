// structuredClone must create independent Date and RegExp instances, including
// when the same source value occurs more than once in the input graph.
const date = new Date(0);
const dateCopy = structuredClone(date);
console.log(dateCopy !== date, dateCopy instanceof Date, dateCopy.getTime());
dateCopy.setTime(5000);
console.log(date.getTime(), dateCopy.getTime());

const invalid = new Date(NaN);
const invalidCopy = structuredClone(invalid);
console.log(invalidCopy !== invalid, invalidCopy instanceof Date, Number.isNaN(invalidCopy.getTime()));

const nested = structuredClone({ left: date, right: date });
console.log(nested.left === nested.right, nested.left !== date, nested.left.getTime());
nested.left.setTime(9000);
console.log(date.getTime(), nested.right.getTime());

const regexp = /a+/gi;
regexp.lastIndex = 3;
const regexpCopy = structuredClone(regexp);
console.log(regexpCopy !== regexp, regexpCopy instanceof RegExp, regexpCopy.source, regexpCopy.flags);
console.log(regexp.lastIndex, regexpCopy.lastIndex);
regexpCopy.lastIndex = 7;
console.log(regexp.lastIndex, regexpCopy.lastIndex);
