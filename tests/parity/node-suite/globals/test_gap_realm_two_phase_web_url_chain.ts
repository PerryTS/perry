class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
const url = new URL("https://example.test/a?x=1&x=2");
const params = url.searchParams;
console.log(params instanceof URLSearchParams, params.getAll("x").join(","));
params.append("y", "3");
console.log(url.search, params === url.searchParams);
