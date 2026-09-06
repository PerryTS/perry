// Wide parse shapes must not stay alive through parser metadata. Keep several
// actual outputs live while dropping the others; those outputs still own and
// trace their keys, including through later writes and stringify.
const fields: string[] = [];
for (let i = 0; i < 5000; i++) fields.push('"k' + i + '":' + i);
const source = '{' + fields.join(',') + '}';
const kept: any[] = [];
let checksum = 0;
for (let round = 0; round < 36; round++) {
    const value: any = JSON.parse(source);
    checksum += value.k0 + value.k127 + value.k4096 + value.k4999;
    if (round % 9 === 0) kept.push(value);
}
let failures = 0;
for (let i = 0; i < kept.length; i++) {
    const value: any = kept[i];
    if (value.k4999 !== 4999 || Object.keys(value).length !== 5000) failures++;
    value.extra = i;
    value.k4096 = -i;
    const copy: any = JSON.parse(JSON.stringify(value));
    if (copy.extra !== i || copy.k4096 !== -i || copy.k4999 !== 4999) failures++;
    if (Object.keys(copy).length !== 5001) failures++;
}
console.log('wide cache lifetime', kept.length, checksum, failures);
