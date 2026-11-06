// HWS companion: date
// HWS hook: js_runtime_install_date
// HWS prefixes: perry_runtime::date::|js_date_
const DateAlias=Date;
const date=new DateAlias("2000-02-29T12:34:56.789Z");
console.log(date.toISOString(),date.getUTCFullYear(),date.getUTCMonth());
const parse=Date.parse;console.log(parse("1970-01-01T00:00:00Z"));
const getTime=Date.prototype.getTime;console.log(getTime.call(date)===date.valueOf());
console.log(JSON.stringify({date}));
