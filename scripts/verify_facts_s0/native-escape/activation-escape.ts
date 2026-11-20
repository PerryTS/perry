const addon = require("./invoke.node");
function body() { console.log("BODY:escape"); }
addon.invoke(body);
