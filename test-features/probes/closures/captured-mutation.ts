let seed: number = 1;

function makeAdder(delta: number) {
  let total = seed;
  return (next: number) => {
    total += delta + next;
    return total;
  };
}

const add = makeAdder(2);
console.log("closures/captured-mutation:" + [add(3), add(4)].join(","));
