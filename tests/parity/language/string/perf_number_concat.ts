const N = Number(process.argv[3] ?? 200000);
const mode = process.argv[2] ?? 'negative';
let sum = 0;
if (mode === 'positive') for (let i=0;i<N;i++) sum += ('number-prefix:'+(1234567+i)).length;
if (mode === 'negative') for (let i=0;i<N;i++) sum += ('number-prefix:'+(-123456-i)).length;
if (mode === 'suffix-positive') for (let i=0;i<N;i++) sum += ((1234567+i)+':number-suffix').length;
if (mode === 'suffix-negative') for (let i=0;i<N;i++) sum += ((-123456-i)+':number-suffix').length;
if (mode === 'chain-positive') for (let i=0;i<N;i++) sum += ('value:'+(1234567+i)+':end').length;
if (mode === 'chain-negative') for (let i=0;i<N;i++) sum += ('value:'+(-123456-i)+':end').length;
if (mode === 'non-number') {const values:any[]=[true,false,null,undefined,'word'];for(let i=0;i<N;i++) sum+=('number-prefix:'+values[i%5]).length;}
console.log(sum);
