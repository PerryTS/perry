function mutatedBound(): number {
  let n = 3;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
    n = 0;
  }
  return count * 10 + n;
}

function fractionalBound(): number {
  let n = 1.5;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
  }
  return count;
}

function nanBound(): number {
  let n = 0 / 0;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
  }
  return count;
}

console.log(mutatedBound() * 10 + fractionalBound() * 5 + nanBound());
