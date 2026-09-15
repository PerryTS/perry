//! #10270: IsArray is not proof that a Proxy id is an ArrayHeader.

mod support;

const SOURCE: &str = r#"
const t = (name: string, f: () => any) => { try { console.log(name, JSON.stringify(f())) } catch (e: any) { console.log(name, "THROW", e.message) } }
const pa = () => new Proxy([["x-a", "1"]], {})
t("Q1 [...proxyOverArray]", () => [...(pa() as any)].length)
t("Q2 for..of proxyOverArray", () => { let n = 0; for (const _ of pa() as any) n++; return n })
t("Q3 Array.isArray(proxyOverArray)", () => Array.isArray(pa()))
t("Q4 proxy[Symbol.iterator] typeof", () => typeof (pa() as any)[Symbol.iterator])
t("Q5 Array.from(proxyOverArray)", () => Array.from(pa() as any).length)
t("nested", () => Array.from(new Proxy(new Proxy([1, 2], {}), {})))
t("trapped indices", () => Array.from(new Proxy([1, 2], { get(t: any, k: any) { return k === "0" ? 9 : t[k]; } })))
t("custom iterator", () => Array.from(new Proxy([1, 2], { get(t: any, k: any) { return k === Symbol.iterator ? () => [7, 8, 9].values() : t[k]; } })))
t("mapped", () => Array.from(new Proxy([1, 2], {}), (v: number, i: number) => v + i))
t("mapped custom iterator", () => Array.from(new Proxy([1, 2], { get(t: any, k: any) { return k === Symbol.iterator ? () => [7, 8, 9].values() : t[k]; } }), (v: number) => v * 2))
t("arraylike", () => Array.from(new Proxy({ 0: "a", 1: "b", length: 2 }, {})))
t("removed iterator", () => Array.from(new Proxy([1, 2], { get(t: any, k: any) { return k === Symbol.iterator ? undefined : t[k]; } })))
t("record iterator", () => Array.from(new Proxy({}, { get(t: any, k: any) { return k === Symbol.iterator ? () => [5, 6].values() : t[k]; } })))
t("headers outer", () => new Headers(new Proxy([["x-a", "1"]], {}) as any).get("x-a"))
t("headers pair", () => new Headers([new Proxy(["x-a", "1"], {})] as any).get("x-a"))
t("plain array", () => Array.from([3, 4]))
t("of preserves proxy", () => { const p = new Proxy([1, 2], {}); return Array.of(p)[0] === p; })
t("concat indices", () => [0].concat(new Proxy([1, 2], { get(t: any, k: any) { return k === Symbol.iterator ? () => [9].values() : k === "0" ? 7 : t[k]; } })))
t("concat holes", () => { const a = [0].concat(new Proxy([, 2], {})); return [a.length, 1 in a, a[2]]; })
t("call spread", () => { const count = (...xs: any[]) => xs.length; return count(...new Proxy([1, 2], {})); })
t("set iterator", () => Array.from(new Set(new Proxy([1, 2], { get(t: any, k: any) { return k === Symbol.iterator ? () => [7, 8].values() : t[k]; } }))))
Promise.all(new Proxy([Promise.resolve(1), Promise.resolve(2)], {})).then(v => console.log("all", JSON.stringify(v)))
"#;

#[test]
fn proxy_array_from_and_headers_iterables() {
    assert_eq!(
        support::compile_and_run(SOURCE),
        concat!(
            "Q1 [...proxyOverArray] 1\n",
            "Q2 for..of proxyOverArray 1\n",
            "Q3 Array.isArray(proxyOverArray) true\n",
            "Q4 proxy[Symbol.iterator] typeof \"function\"\n",
            "Q5 Array.from(proxyOverArray) 1\n",
            "nested [1,2]\n",
            "trapped indices [9,2]\n",
            "custom iterator [7,8,9]\n",
            "mapped [1,3]\n",
            "mapped custom iterator [14,16,18]\n",
            "arraylike [\"a\",\"b\"]\n",
            "removed iterator [1,2]\n",
            "record iterator [5,6]\n",
            "headers outer \"1\"\n",
            "headers pair \"1\"\n",
            "plain array [3,4]\n",
            "of preserves proxy true\n",
            "concat indices [0,7,2]\n",
            "concat holes [3,false,2]\n",
            "call spread 2\n",
            "set iterator [7,8]\n",
            "all [1,2]\n",
        )
    );
}

#[test]
fn proxy_array_from_reads_iterator_methods_once_and_closes_on_mapping_throw() {
    let source = r#"
let reads = 0;
const p = new Proxy([1, 2], { get(t: any, k: any) {
  if (k === Symbol.iterator) { reads++; return reads === 1 ? function() { return [4, 5].values(); } : undefined; }
  return t[k];
}});
console.log("method", JSON.stringify(Array.from(p)), reads);
let nextReads = 0, steps = 0, closed = 0;
const it = { get next() { nextReads++; return function() { return steps++ < 2 ? {done:false, value:7} : {done:true}; }; }, return() { closed++; return {done:true}; } };
const q = new Proxy({}, {get(t:any,k:any) { return k === Symbol.iterator ? () => it : undefined; }});
console.log("next", JSON.stringify(Array.from(q)), nextReads);
steps = 0;
try { Array.from(q, () => { throw new Error("mapped"); }); } catch(e:any) { console.log("close",e.message,closed); }
let selected = false;
const r = new Proxy([1], {get(t:any,k:any) { if(k === Symbol.iterator) { selected = true; return function() { return [8].values(); }; } return t[k]; }});
function C() { console.log("constructor", selected); }
const result:any = Array.from.call(C, r);
console.log("constructed", result[0], result.length);
"#;
    assert_eq!(
        support::compile_and_run(source),
        "method [4,5] 1\nnext [7,7] 1\nclose mapped 1\nconstructor true\nconstructed 8 1\n"
    );
}
