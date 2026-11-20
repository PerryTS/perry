// Even smaller
const fn = () => {
    let index = -1;
    return dispatch(0);
    async function dispatch(i: number): Promise<number> {
        index = i;
        return index;
    }
};

async function main() {
    const r = await fn();
    console.log("r:", r);
}
main();
