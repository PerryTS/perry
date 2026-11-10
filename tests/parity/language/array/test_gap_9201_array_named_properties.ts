const arr: any[] = [1, 2, 3];
arr.foo = "bar";
console.log("before", arr.length, arr.foo, Object.keys(arr).join(","));

arr[10] = "sparse";

console.log("after", arr.length, arr.foo, arr[10]);
console.log(Object.keys(arr).join(","));

const dynamic: any = [4, 5];
dynamic.metadata = { total: 2 };
dynamic["hasMore"] = false;
console.log("before", dynamic.length, dynamic.metadata.total, dynamic.hasMore);

dynamic[12] = "tail";

console.log("after", dynamic.length, dynamic.metadata.total, dynamic.hasMore, dynamic[12]);
console.log(Object.keys(dynamic).join(","));
console.log(JSON.stringify(dynamic));
