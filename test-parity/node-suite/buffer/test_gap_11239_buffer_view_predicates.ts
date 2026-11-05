import { types } from 'node:util';

const isView = ArrayBuffer.isView;
const isUtilView = types.isArrayBufferView;
const isUint8 = types.isUint8Array;
const isTyped = types.isTypedArray;
function check(label: string, value: any) {
  console.log(label, ArrayBuffer.isView(value), isView(value),
    types.isArrayBufferView(value), isUtilView(value),
    types.isUint8Array(value), isUint8(value),
    types.isTypedArray(value), isTyped(value), types.isInt8Array(value));
}
const buffer = Buffer.alloc(4);
check('alloc', buffer);
check('empty', Buffer.alloc(0));
check('from', Buffer.from('abc'));
check('subarray', buffer.subarray(1, 3));
check('slice', buffer.slice(1, 3));
check('backing', Buffer.from(new ArrayBuffer(32), 8, 16));
check('uint8', new Uint8Array(4));
check('int8', new Int8Array(4));
check('uint16', new Uint16Array(4));
check('dataview', new DataView(new ArrayBuffer(4)));
check('arraybuffer', new ArrayBuffer(4));
check('shared', new SharedArrayBuffer(4));
check('array', [1, 2]);
check('object', { byteLength: 16 });
check('null', null);
check('undefined', undefined);
check('number', 42);
check('string', 'abc');
// bson's UUID constructor uses this view check before accepting a byte span.
const bytes = Buffer.from(new ArrayBuffer(32), 8, 16).subarray(0, 16);
console.log('uuid bytes', ArrayBuffer.isView(bytes) && bytes.byteLength === 16);
console.log('instanceof', buffer instanceof Uint8Array);
