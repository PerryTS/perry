class S extends Set{add(v){return super.add(v)} each(f){return super.forEach(f)}} const s=new S(); console.log(s.add(2)===s); s.each((v,k,o)=>console.log(v,k,o===s));
