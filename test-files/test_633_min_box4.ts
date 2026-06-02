// Same shape as mb3 but SYNC dispatch (not async) to isolate
const fn = () => {
    let index = -1;
    return dispatch(7);
    function dispatch(i: number): number {
        return i + index;
    }
};

const r = fn();
console.log("r:", r);
