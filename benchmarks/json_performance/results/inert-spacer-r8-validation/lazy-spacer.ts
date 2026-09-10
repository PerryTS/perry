let source = '[';
for (let i = 0; i < 180; i++) {
    if (i) source += ',';
    source += '{ "id": 0, "id": ' + i + ', "name": "Ada", "tags": [1,2] }';
}
source += ']';
const array: any = JSON.parse(source);
const spaces: any[] = [undefined, 0, -0, true, 2];
for (let i = 0; i < spaces.length; i++) console.log('array', i, JSON.stringify(array, null, spaces[i]));
array[3].id = 999;
for (let i = 0; i < spaces.length; i++) console.log('mutated', i, JSON.stringify(array, null, spaces[i]));
array.toJSON = function(key: string) {return {length: array.length, key: key};};
for (let i = 0; i < spaces.length; i++) console.log('own-toJSON', i, JSON.stringify(array, null, spaces[i]));
