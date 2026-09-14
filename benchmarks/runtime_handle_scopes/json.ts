const source = '{"name":"a-long-heap-string-payload","count":42,"items":[1,2,3],"child":{"ok":true}}';
let checksum = 0;
for (let i = 0; i < 100000; i++) {
  const value = JSON.parse(source);
  value.count = i;
  const encoded = JSON.stringify(value);
  const decoded = JSON.parse(encoded);
  checksum += encoded.length + decoded.count + decoded.items[2];
}
console.log(checksum);
