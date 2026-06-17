function makeAdder(seed: number) {
    let total = seed;
    return (value: number) => {
        total += value;
        return total;
    };
}

const add = makeAdder(10);
console.log(`closure:${add(5)}:${add(7)}`);
