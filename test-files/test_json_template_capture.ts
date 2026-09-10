const texts = [
    '{"padding":"enough-padding-to-enter-the-small-object-template-path","items":["alpha-value",2,true],"id":7}',
    '{"padding":"another-padding-to-enter-the-small-object-template-path","items":["beta-value",false,3],"id":8}',
    '{"padding":"different-padding-to-enter-the-small-object-template-path","items":["gamma-value",4,null],"id":9}'
];
const saved: any[] = [];
let sum = 0;
for (let i = 0; i < 240; i++) {
    const text = texts[i % 3];
    const first: any = JSON.parse(text);
    // An ineligible plan must not publish a partially captured prefix.
    const rejectedText = '{"padding":"enough-padding-to-enter-the-small-object-template-path","items":["new",3],"nested":{"id":1}}';
    const rejected: any = JSON.parse(rejectedText);
    if (JSON.stringify(rejected) !== rejectedText) throw new Error('rejected capture changed output');
    const second: any = JSON.parse(text);
    first.items[0] = 'changed';
    first.items.push(99);
    if (second.items.length !== 3 || second.items[0] === 'changed') {
        throw new Error('cached mutable array was shared');
    }
    const third: any = JSON.parse(text);
    if (JSON.stringify(second) !== text || JSON.stringify(third) !== text) {
        throw new Error('cached template changed');
    }
    if (i % 23 === 0) saved.push({value: second, expected: text});
    const pressure: any[] = [];
    for (let j = 0; j < 80; j++) pressure.push({id: j, name: 'pressure-' + j});
    sum += third.id + pressure[79].id;
}
for (let i = 0; i < saved.length; i++) {
    const item: any = saved[i];
    if (JSON.stringify(item.value) !== item.expected) throw new Error('retained value changed');
    console.log('saved', i, item.value.id, item.value.items[0]);
}
console.log('done', sum, saved.length);
