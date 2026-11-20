class N extends null{m(){return super.x}} try{console.log(N.prototype.m.call({}))}catch(e){console.log(e instanceof TypeError)}
