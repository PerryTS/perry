// parity-node-argv: --expose-gc
// Header arrays are owned by native responses through traced JS state, including
// values installed after the owner survived collections and values after close.
import * as http from 'node:http';
declare function gc(): void;
const responses:any[]=[];
for(let i=0;i<32;i++)responses.push(i%2===0?new http.ServerResponse({method:'GET'} as any):new http.OutgoingMessage());
for(let round=0;round<3;round++)if(typeof gc==='function')gc();
for(let i=0;i<responses.length;i++){
 const values=['header-value-'+i+'-long-enough-to-allocate'];
 responses[i].setHeader('X-Test',values);responses[i].setHeader('Num',i);
 for(let j=0;j<12;j++)responses[i].setHeader('X-Grow'+j,['growing-value-'+i+'-'+j+'-long-enough-to-allocate']);
}
for(let round=0;round<8;round++){
 const churn:any[]=[];for(let i=0;i<500;i++)churn.push({name:'churn-value-'+round+'-'+i,items:[i,round]});
 if(typeof gc==='function')gc();
 for(let i=0;i<responses.length;i++){
  const r=responses[i];const a=r.getHeader('x-test');
  for(let j=0;j<12;j++)if(r.getHeader('x-grow'+j)[0]!=='growing-value-'+i+'-'+j+'-long-enough-to-allocate')throw new Error('lost grown slot');
  if(a[0]!=='header-value-'+i+'-long-enough-to-allocate')throw new Error('lost header');
  if(r.getHeaders()['x-test']!==a)throw new Error('lost identity');
  if(round===2)r.destroy();
  r.appendHeader('X-Test','r'+round);
  if(r.getHeader('x-test')!==a)throw new Error('append identity');
 }
}
console.log(responses[31].getHeader('x-test').join('|'));
console.log(responses[31].getHeader('num'),responses[31].getHeaderNames().join(','));

const copies:any[]=responses.map((r:any)=>r.getHeaders());
responses.length=0;
for(let round=0;round<3;round++)if(typeof gc==='function')gc();
for(let i=0;i<copies.length;i++){
 if(copies[i]['x-test'][0]!=='header-value-'+i+'-long-enough-to-allocate'||copies[i]['num']!==i)throw new Error('lost copied value');
 for(let j=0;j<12;j++)if(copies[i]['x-grow'+j][0]!=='growing-value-'+i+'-'+j+'-long-enough-to-allocate')throw new Error('lost copied grown slot');
}
console.log('copies',copies.length);

const typed=new http.ServerResponse({method:'GET'} as any);
typed.setHeader('X-Typed',['typed-header-value-long-enough-to-allocate']);
const typedValue=typed.getHeader('x-typed') as any;
const typedCopy=typed.getHeaders();
const typedNames=typed.getHeaderNames();
typed.destroy();
for(let round=0;round<3;round++)if(typeof gc==='function')gc();
console.log('typed',typed.getHeader('x-typed')===typedValue,typedValue[0],typedCopy['x-typed']===typedValue,typedNames.join(','));
