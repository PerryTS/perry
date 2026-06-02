class Router2 {
    match(method: string, path: string): string {
        return `Router2:${method}:${path}`;
    }
}

const router = new Router2();
const m = router.match;
console.log("typeof m =", typeof m);
console.log("m direct:", m("GET", "/test"));

const bound = router.match.bind(router);
console.log("typeof bound =", typeof bound);
console.log("bound direct:", bound("GET", "/test"));
