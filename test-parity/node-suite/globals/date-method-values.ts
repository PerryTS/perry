const d = new Date(0);

function log(label: string, value: unknown) {
  console.log(`${label}:`, String(value));
}

function logErrorName(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(`${label}: no-error`);
  } catch (err) {
    console.log(`${label}:`, (err as Error).name);
  }
}

const opaque: any = d;
const typedGetTime = d.getTime;
const typedToISOString = d.toISOString;
const typedToJSON = d.toJSON;
const getTime = opaque.getTime;
const getTimeByIndex = opaque["getTime"];
const toISOString = opaque.toISOString;
const toJSON = opaque.toJSON;
const setTime = opaque.setTime;
const valueOf = opaque.valueOf;

log("typed typeof getTime", typeof typedGetTime);
log("typed typeof toISOString", typeof typedToISOString);
log("typed typeof toJSON", typeof typedToJSON);
log("typed getTime identity", typedGetTime === Date.prototype.getTime);
log("typed toISOString identity", typedToISOString === Date.prototype.toISOString);
log("typed toJSON identity", typedToJSON === Date.prototype.toJSON);
log("typed getTime call", typedGetTime.call(d));
log("typed toISOString call", typedToISOString.call(d));
log("typed toJSON call", typedToJSON.call(d));

log("opaque typeof getTime", typeof getTime);
log("opaque typeof toISOString", typeof toISOString);
log("opaque typeof toJSON", typeof toJSON);
log("getTime identity", getTime === Date.prototype.getTime);
log("getTime index identity", getTimeByIndex === Date.prototype.getTime);
log("toISOString identity", toISOString === Date.prototype.toISOString);
log("toJSON identity", toJSON === Date.prototype.toJSON);
log("valueOf identity", valueOf === Date.prototype.valueOf);
log("getTime call", getTime.call(d));
log("getTime index call", getTimeByIndex.call(d));
log("toISOString call", toISOString.call(d));
log("toJSON call", toJSON.call(d));
log("valueOf call", valueOf.call(d));

const originalGetTime = Date.prototype.getTime;
(Date.prototype as any).getTime = function () {
  return 77;
};
const overriddenGetTime = opaque.getTime;
log("overridden getTime identity", overriddenGetTime === Date.prototype.getTime);
log("overridden getTime call", overriddenGetTime.call(d));
Date.prototype.getTime = originalGetTime;

log("setTime call", setTime.call(d, 1234));
log("after setTime", d.getTime());
logErrorName("new toJSON", () => new opaque.toJSON());
