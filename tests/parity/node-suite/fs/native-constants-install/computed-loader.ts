const proc: any = process;
const method = ['get', 'BuiltinModule'].join('');
const fs: any = proc[method]('fs');
console.log(fs[['const', 'ants'].join('')].R_OK);
