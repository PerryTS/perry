// Issue #637 followup / hono r2: closure-captured array IndexSet
// pre-fix used js_array_set_f64 (non-extending) which silently no-op'd
// when index >= length. v0.5.737 switches to js_array_set_f64_extend.
function go() {
    const arr: any[] = [];
    const fn = () => {
        arr[0] = "a";
        arr[2] = "c"; // sparse extend — gap at index 1
    };
    fn();
    console.log("len:", arr.length);
    console.log("arr[0]:", arr[0]);
    console.log("arr[1]:", arr[1]);
    console.log("arr[2]:", arr[2]);
    console.log("json:", JSON.stringify(arr));
}
go();

// Issue #637 / hono Trie pattern: arr[++captureIndex] = N inside replace cb
function build() {
    const indexReplacementMap: any[] = [];
    let captureIndex = 0;
    const input = "/users/([^/]+)@0#0";
    input.replace(/#(\d+)|@(\d+)/g, (_, h, p) => {
        if (h !== undefined) {
            indexReplacementMap[++captureIndex] = Number(h);
            return "$()";
        }
        if (p !== undefined) {
            ++captureIndex;
            return "";
        }
        return "";
    });
    console.log("irm len:", indexReplacementMap.length);
    console.log("irm:", JSON.stringify(indexReplacementMap));
}
build();

// Issue #637: arr[stringKey] = X coercion via direct closure call
function strKey() {
    const out: any[] = [];
    const fn = (k: string) => {
        out[k as any] = "h" + k;
    };
    fn("0");
    fn("1");
    fn("2");
    console.log("out len:", out.length);
    console.log("out:", JSON.stringify(out));
}
strKey();

// Issue #637 followup: forEach-captured array, Any-typed param key.
// Pre-fix the fast path's fptosi(NaN-string,i32) collapsed all writes
// to slot 0; codegen now routes Any-keyed array IndexSet through
// js_array_set_index_or_string which detects string tags at runtime.
function forEachKey() {
    const out: any[] = [];
    const keys = ["0", "1", "2"];
    keys.forEach((k) => {
        out[k as any] = "fk" + k;
    });
    console.log("foreach out len:", out.length);
    console.log("foreach out:", JSON.stringify(out));
}
forEachKey();
