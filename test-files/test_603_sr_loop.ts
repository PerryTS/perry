class SubRouter {
    match(method: string, path: string): string {
        return `Sub:${method}:${path}`;
    }
    add(method: string, path: string, handler: any): void {}
}

class SmartRouter {
    #routes: any[] | undefined = [];
    #routers: SubRouter[] = [new SubRouter()];

    match(method: string, path: string): string {
        console.log("[SR.match] orig", method, path);
        if (!this.#routes) {
            throw new Error("Fatal error");
        }
        const routers = this.#routers;
        const routes = this.#routes;
        const len = routers.length;
        let i = 0;
        let res = "";
        for (; i < len; i++) {
            const router = routers[i];
            try {
                for (let i2 = 0; i2 < routes.length; i2++) {
                    router.add(routes[i2][0], routes[i2][1], routes[i2][2]);
                }
                res = router.match(method, path);
            } catch (e) {
                throw e;
            }
            this.match = router.match.bind(router);
            this.#routers = [router];
            this.#routes = void 0;
            break;
        }
        if (i === len) {
            throw new Error("Fatal error");
        }
        return res;
    }
}

const sr = new SmartRouter();
try {
    console.log("r1:", sr.match("GET", "/"));
    console.log("r2:", sr.match("GET", "/x"));
    console.log("r3:", sr.match("GET", "/y"));
} catch (e) {
    console.log("CAUGHT:", (e as any).message);
}
