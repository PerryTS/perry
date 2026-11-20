const name = process.argv.length > 0 ? 'node:fs' : 'node:os';
const fs: any = process.getBuiltinModule(name);
const member = ['const', 'ants'].join('');
const key = ['R', 'OK'].join('_');
console.log(fs[member][key]);
const crypto: any = process.getBuiltinModule(['node', 'crypto'].join(':'));
console.log(crypto[member][['RSA', 'PKCS1', 'PADDING'].join('_')]);
const legacy: any = process.getBuiltinModule(['node', 'constants'].join(':'));
console.log(legacy[key], legacy[['RSA', 'PKCS1', 'PADDING'].join('_')]);
