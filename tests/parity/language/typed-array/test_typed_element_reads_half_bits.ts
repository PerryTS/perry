// Every binary16 encoding, including subnormal/normal boundaries and NaNs.
function checksum(a: Float16Array): void {
 let finite=0, nan=0, negativeZero=0, inf=0;
 for(let i=0;i<a.length;i++) {
  const v=a[i];
  if(Number.isNaN(v)) nan++;
  else if(!Number.isFinite(v)) inf++;
  else { finite += Math.abs(v); if(Object.is(v,-0)) negativeZero++; }
 }
 console.log(finite,nan,negativeZero,inf);
}
const raw=new Uint16Array(65536); for(let i=0;i<raw.length;i++) raw[i]=i;
checksum(new Float16Array(raw.buffer));
