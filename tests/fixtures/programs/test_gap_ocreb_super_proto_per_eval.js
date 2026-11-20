function mk(P){ return class extends P {} }
mk(function(){});
try { mk(function(){}.bind(null)); console.log("no throw") } catch(e){ console.log(e instanceof TypeError) } // true
function F(){}; F.prototype.w=()=>"F"; function G(){}; G.prototype.w=()=>"G";
mk(F); console.log(new (mk(G))().w()) // G
