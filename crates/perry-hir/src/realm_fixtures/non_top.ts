import fs from 'node:fs';
if (typeof Map === 'undefined') throw new Error('Map');
globalThis.Map; global.Set; globalThis['Promise'];
require('node:fs'); import('node:fs');
const regex = /x/g;
async function f() { await Promise.resolve(1); }
class M extends Map {}
function* g() { yield 1; }
async function* ag() { for await (const item of []) yield item; }
const spread = [...[1, 2]];
const [first] = [1];
for (const item of 'text') console.log(item);
Map[key] = custom;

