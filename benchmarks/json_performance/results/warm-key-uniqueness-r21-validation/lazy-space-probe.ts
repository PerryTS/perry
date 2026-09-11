const count = Number(process.argv[2]);
const form = process.argv[3];
const mode = process.argv[4];
let source = '[';
for (let i = 0; i < count; i++) {
    if (i) source += ',';
    if (form === 'canonical') source += '{"id":' + i + ',"name":"Ada","tags":[1,2]}';
    else if (form === 'whitespace') source += '{ "id": ' + i + ', "name": "Ada", "tags": [1,2] }';
    else source += '{"id":0,"id":' + i + ',"name":"Ada","tags":[1,2]}';
}
source += ']';
const value: any = JSON.parse(source);
let space: any = undefined;
if (mode === 'zero') space = 0;
else if (mode === 'true') space = true;
else if (mode === 'pretty') space = 2;
console.log('before');
console.log('out', JSON.stringify(value, null, space));
console.log('done');
