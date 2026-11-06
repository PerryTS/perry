// Override pattern via type-erased receiver — dynamic dispatch path
class Router {
    match(method: string, path: string): string {
        console.log("[orig] match called");
        const inner = (m: string, p: string) => `inner:${m}:${p}`;
        this.match = inner;
        return inner(method, path);
    }
}

interface Matcher {
    match(method: string, path: string): string;
}

const r: Matcher = new Router();
console.log("r1:", r.match("GET", "/"));
console.log("r2:", r.match("GET", "/x"));
console.log("r3:", r.match("GET", "/y"));
