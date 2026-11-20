// Issue #620: this.method = X should override class dispatch
class Router {
    routes: any[] | undefined = [];
    match(path: string): string {
        console.log("[Router.match] original called");
        const inner = (p: string) => `inner:${p}`;
        this.match = inner;
        return inner(path);
    }
}

const r = new Router();
console.log(r.match("/foo"));
console.log(r.match("/bar"));
