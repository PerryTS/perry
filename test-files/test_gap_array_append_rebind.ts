'use strict';
let globalArray: any[] = [];
for (let i = 0; i < 8; i++) globalArray.push(i);
const globalAlias = globalArray;
function replaceGlobal() { globalArray = [77]; return 99; }
globalArray[globalArray.length] = replaceGlobal();
console.log('global', globalArray.length, globalArray[0], globalAlias.length, globalAlias[8]);
function captured() {
    let a: any[] = [];
    for (let i = 0; i < 8; i++) a.push(i);
    const alias = a;
    function replace() { a = [77]; return 99; }
    a[a.length] = replace();
    console.log('captured', a.length, a[0], alias.length, alias[8]);
}
captured();
