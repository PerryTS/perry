const {Writable}=require("stream"); const w=new Writable({write(c,e,cb){this.seen=1; cb()}}); w.write("x"); console.log(w.seen) // 1
