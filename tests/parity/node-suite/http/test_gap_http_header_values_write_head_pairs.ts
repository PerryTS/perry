import * as http from "node:http";
for (const prior of [false,true]) {
 const r:any=new http.ServerResponse({method:'GET'} as any); r.sendDate=false;
 if(prior)r.setHeader('Before',1);
 r.writeHead(200,['X-Test','one','X-Test','two','Set-Cookie','a=1','Set-Cookie','b=2','Num',23]);
 console.log(prior,JSON.stringify(r.getHeaders()),r.getHeaderNames().join(','));
 console.log(r._header.includes('X-Test: one\r\nX-Test: two\r\n'),r._header.includes('Set-Cookie: a=1\r\nSet-Cookie: b=2\r\n'));
}
