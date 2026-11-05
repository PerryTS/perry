import { StringDecoder } from 'node:string_decoder';
const symbol = Symbol('owner');
const decoder: any = new StringDecoder('utf8');
decoder.payload = { value: 73 };
decoder[symbol] = { value: 41 };
Object.defineProperty(decoder, 'hidden', { value: 17, configurable: true });
Object.defineProperty(decoder, 'answer', { get: () => decoder.payload.value, enumerable: true, configurable: true });
for (let i = 0; i < 40000; i++) { const p: any = { i, text: 'pressure' + i }; if (p.i < 0) console.log(p); }
console.log(decoder.payload.value, decoder[symbol].value, decoder.hidden, decoder.answer);
console.log(Object.getOwnPropertyDescriptor(decoder, 'hidden')!.enumerable);
console.log(Object.getOwnPropertySymbols(decoder).includes(symbol));
console.log(delete decoder.hidden, decoder.hidden === undefined);
const headers: any = new Headers();
headers.payload = { value: 29 };
Object.defineProperty(headers, 'computed', { get: () => headers.payload.value + 1, configurable: true });
const blob: any = new Blob(['text']);
blob.payload = { value: 31 };
for (let i = 0; i < 40000; i++) { const p: any = { i, text: 'later' + i }; if (p.i < 0) console.log(p); }
console.log(headers.payload.value, headers.computed, blob.payload.value, blob.size);
console.log(new StringDecoder('utf8').payload === undefined, new Headers().payload === undefined);
