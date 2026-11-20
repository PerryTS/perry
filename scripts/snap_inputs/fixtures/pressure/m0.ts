const keep: any[] = [];
for (let i = 0; i < 1000; i++) {
 const proto: any = { marker: i };
 const obj: any = Object.create(proto);
 obj.x = i;
 Object.defineProperty(obj, "y", {value: i + 1, enumerable: true});
 keep.push(obj);
}
export const value = keep.length;
