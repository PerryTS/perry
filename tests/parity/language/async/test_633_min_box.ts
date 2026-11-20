// Even smaller — just the dispatch chain shape
const compose = () => {
    return (ctx: any) => {
        let index = -1;
        return dispatch(0);
        async function dispatch(i: number): Promise<any> {
            index = i;
            return ctx;
        }
    };
};

const composed = compose();
async function main() {
    const r = await composed({ x: 1 });
    console.log("r:", r);
}
main();
