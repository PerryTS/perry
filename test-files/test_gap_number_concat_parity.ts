const values:any[]=[-123456,-0,-1,-1.25,-1e-7,-1e21,-Infinity,Infinity,NaN,0,1,1.25,1e-7,1e21,true,false,null,undefined,'word',{toString(){return 'object';}}];
for(const value of values) console.log('number-prefix:'+value,value+':number-suffix','v:'+value+':end');
const utf='🦆é';
for(const value of [-123456,-Infinity,NaN,false]) console.log(utf+value,value+utf);
let n=0;const object={toString(){n++;return 'object';}};
console.log('number-prefix:'+object,object+':number-suffix','v:'+object+':end',n);
let total=0;
for(let i=0;i<25000;i++) {
  const prefix='long-🐣-'+i;
  const value=-123456-i;
  total+=(prefix+value).length;
  total+=(value+prefix).length;
}
console.log('moving-prefix-total='+total);
