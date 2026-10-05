import {AsyncLocalStorage} from 'node:async_hooks';
const storage=new AsyncLocalStorage<string>();
let closed=0;let observed='';
function* gen(){try{yield 1;yield 2;}finally{closed++;observed=storage.getStore()||'none';}}
storage.run('outer',()=>{
 try{for(const value of gen()){storage.run('inner',()=>{throw 'body';});}}
 catch(e){console.log('context-close',e,closed,observed,storage.getStore());}
});
let error:any={marker:'original'};let saved:any;
function churn(){const a:any[]=[];for(let j=0;j<12000;j++)a.push({j,text:'payload-'+j});return a[11999].j;}
function* pressure(){try{yield {marker:'value'};yield 2;}finally{churn();closed++;}}
closed=0;try{for(const value of pressure()){saved=value;churn();throw error;}}
catch(e:any){console.log('pressure-close',e===error,e.marker,saved.marker,closed);}
const user:any={next(){return {done:false,value:1};},[Symbol.iterator](){return this;},
 return(){churn();closed++;throw {marker:'close'};}};
closed=0;try{for(const value of user){churn();throw error;}}
catch(e:any){console.log('pressure-original-throw',e===error,e.marker,closed);}
