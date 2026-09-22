function run(n: any, O: any) {
    let h = 0;
    for (let k = 0; k < n; k++) h += O.a + O.b;
    return h;
}
const literal = {a: 1, b: 2};
const grown: any = {};
grown.a = 1;
grown.b = 2;
console.log(run(100, literal), run(100, grown));
