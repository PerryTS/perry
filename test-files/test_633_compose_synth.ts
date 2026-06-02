// Synthetic compose pattern from hono — does the dispatch fire the handler?
const compose = (middleware: any[], onError: any, onNotFound: any) => {
    return (context: any, next: any) => {
        let index = -1;
        return dispatch(0);
        async function dispatch(i: number): Promise<any> {
            if (i <= index) throw new Error("next() multiple times");
            index = i;
            let res: any;
            let handler: any;
            if (middleware[i]) {
                handler = middleware[i][0][0];
            } else {
                handler = i === middleware.length && next || undefined;
            }
            if (handler) {
                try {
                    res = await handler(context, () => dispatch(i + 1));
                } catch (err) {
                    if (onError) {
                        res = await onError(err, context);
                    } else throw err;
                }
            } else if (context.finalized === false && onNotFound) {
                res = await onNotFound(context);
            }
            if (res && (context.finalized === false)) {
                context.res = res;
            }
            return context;
        }
    };
};

// Mimic hono's matchResult shape: [[[handler, paramsMap]], emptyParam]
const handler = (c: any, _n: any) => {
    console.log("[handler] fired");
    c.finalized = true;
    return "RES";
};

const middleware = [[[handler, {}]]];
const c = { res: null as any, finalized: false };
const composed = compose(middleware, null, null);

async function main() {
    const ctx = await composed(c, null);
    console.log("ctx.res:", ctx.res);
    console.log("ctx.finalized:", ctx.finalized);
}
main().catch(e => console.log("CAUGHT:", (e as any).message));
