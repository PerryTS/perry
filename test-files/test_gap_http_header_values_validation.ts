import * as http from 'node:http';
const r:any=new http.ServerResponse({method:'GET'} as any);
for(const [name,value] of [['',1],['bad name',1],['x',undefined],['x','a\nb']]){
 for(const method of ['setHeader','appendHeader']){
  try{if(method==='setHeader')r.setHeader(name,value);else r.appendHeader(name,value);console.log('no error')}catch(e:any){console.log(method,e.code)}
 }
}
