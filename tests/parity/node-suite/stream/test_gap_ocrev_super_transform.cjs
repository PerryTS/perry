const {Transform}=require("stream"); class T extends Transform{_transform(c,e,cb){return super._transform(c,e,cb)}} const t=new T(); try{t.write("x")}catch(e){console.log(e.code)}
