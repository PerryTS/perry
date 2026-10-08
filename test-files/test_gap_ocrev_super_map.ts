class M extends Map{set(k,v){return super.set(k,v)}} const m=new M(); console.log(m.set("x",1)===m,m.get("x"));
