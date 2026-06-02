// No mutation — just read of the closure-captured let
const fn = () => {
    let index = -1;
    return dispatch(7);
    async function dispatch(i: number): Promise<number> {
        return i + index;
    }
};

async function main() {
    const r = await fn();
    console.log("r:", r);
}
main();
