const arr = [[1, 2, 3]];
const row = arr[0];

console.log("value", JSON.stringify(row), row === arr[0], Array.isArray(row));

function getRow(): number[] {
  return arr[0];
}

getRow()[1] = 99;
console.log("returned-write", arr[0][1], JSON.stringify(arr));

const compound = [[1, 2, 3]];
compound[0][1] += 10;
console.log("compound", compound[0][1], JSON.stringify(compound));
