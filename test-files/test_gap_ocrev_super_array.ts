class A extends Array{push(...x){return super.push(...x)}} const a=new A(); console.log(a.push(1,2),a.join(","));
