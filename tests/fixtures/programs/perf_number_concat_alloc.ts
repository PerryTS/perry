const value = Number(process.argv[2] ?? 1234567);
function append(value:number) {return 'number-prefix:'+value;}
console.log('warm='+append(value).length+','+append(-value).length);
console.log('heap-warm='+Boolean(process.memoryUsage().heapUsed)+','+Boolean(process.memoryUsage().heapUsed));
const before=process.memoryUsage().heapUsed;
const positive=append(value);
const middle=process.memoryUsage().heapUsed;
const negative=append(-value);
const after=process.memoryUsage().heapUsed;
const positiveBytes=middle-before,negativeBytes=after-middle;
console.log('bytes='+positiveBytes+','+negativeBytes);
console.log(positive,negative);
if(negativeBytes!==positiveBytes) throw new Error('negative numeric concatenation allocated a temporary string');
console.log('ok');
