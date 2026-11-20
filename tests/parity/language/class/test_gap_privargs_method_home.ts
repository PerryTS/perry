// Method function objects must retain their defining evaluation independently
// of the receiver and any enclosing method's private context.
class Root {
    #x = 5;
    #m(v: number) { return this.#x + v; }
    read(v: number) { return this.#m(v); }
    static through(f: () => number) { return f(); }
}
const root = new Root();
const extracted = root.read;
const bound = root.read.bind(root);
console.log('declared', extracted.call(root, 2), bound(3));
(root as any).extra = 1;
Object.freeze(root);
console.log('shape', bound(4));
try { extracted.call({ '#x': 50 }, 1); } catch (e) { console.log('wrong', e instanceof TypeError); }
function factory(n: number) {
    return class Fresh extends Root {
        #x = n;
        own() { return this.#x; }
        read(v: number) { return super.read(v) + this.#x; }
        callback(f: () => number) { return f() + this.#x; }
        static invoke(f: () => number) { return Root.through(f); }
    };
}
const A = factory(11);
const B = factory(23);
const a = new A();
const b = new B();
console.log('fresh', a.read(1), b.read(1), a.callback(() => bound(1)));
console.log('nested', a.callback(() => b.callback(() => bound(2))));
console.log('static', A.invoke(() => bound(3)));
const ao = a.own;
const bo = b.own;
console.log('homes', ao.call(a), bo.call(b));
try { ao.call(b); } catch (e) { console.log('cross', e instanceof TypeError); }
try { a.callback(() => ao.call(b)); } catch (e) { console.log('throw', e instanceof TypeError); }
console.log('restored', bo.call(b), bound(2));
// A nested ordinary method containing an arrow must preserve the derived
// constructor's active this binding across calls and exceptions.
class Leaf extends Root {
    make() { return () => this.read(3); }
}
const leaf = new Leaf();
console.log('arrow', leaf.make()());
