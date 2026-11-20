// Mock of hono's SmartRouter pattern with private field
class SubRouter {
    match(method: string, path: string): string {
        return `Sub:${method}:${path}`;
    }
}

class SmartRouter {
    #routes: any[] | undefined = [];
    match(method: string, path: string): string {
        console.log("[SR.match] orig", method, path);
        if (!this.#routes) {
            throw new Error("Fatal error");
        }
        const sub = new SubRouter();
        const res = sub.match(method, path);
        this.match = sub.match.bind(sub);
        this.#routes = void 0;
        return res;
    }
}

const sr = new SmartRouter();
console.log("r1:", sr.match("GET", "/"));
console.log("r2:", sr.match("GET", "/x"));
console.log("r3:", sr.match("GET", "/y"));
