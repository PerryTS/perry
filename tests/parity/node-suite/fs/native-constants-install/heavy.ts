import * as fs from 'node:fs';
import * as os from 'node:os';
import * as crypto from 'node:crypto';
const groups: any[] = [fs.constants, os.constants.errno, crypto.constants];
const keys = [
  ['F_OK', 'R_OK', 'W_OK', 'X_OK', 'O_RDONLY', 'O_WRONLY', 'O_RDWR', 'COPYFILE_EXCL'],
  ['EACCES', 'EINVAL', 'ENOENT', 'EEXIST', 'EBADF', 'EISDIR', 'ENOTDIR', 'EPERM'],
  ['RSA_PKCS1_PADDING', 'RSA_NO_PADDING', 'RSA_PKCS1_OAEP_PADDING', 'RSA_PKCS1_PSS_PADDING',
   'RSA_PSS_SALTLEN_DIGEST', 'RSA_PSS_SALTLEN_MAX_SIGN', 'POINT_CONVERSION_COMPRESSED',
   'POINT_CONVERSION_UNCOMPRESSED'],
];
let total = 0;
for (let i = 0; i < 10000; i++) {
  for (let j = 0; j < groups.length; j++) {
    for (let k = 0; k < keys[j].length; k++) total += groups[j][keys[j][k]];
  }
}
console.log(total);
