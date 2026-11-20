// Ordinary objects and collection methods must keep their generic paths.
const item: any = {href: "first", pathname: "a", search: "x"};
item.href = "second"; item.pathname = "b"; item.search = "y";
console.log(item.href, item.pathname, item.search);
const values: any = new Map(); values.set("a", "1"); values.set("a", "2");
console.log(values.get("a"), values.size);
console.log(String({toString() { return "ordinary"; }}));
