import { TransformStream, CountQueuingStrategy } from "node:stream/web";
// TransformStream(transformer, writableStrategy, readableStrategy)
// — full 3-arg form sets HWM on both sides.
const ws = new CountQueuingStrategy({ highWaterMark: 1 });
const rs = new CountQueuingStrategy({ highWaterMark: 4 });
const ts = new TransformStream(undefined, ws, rs);
console.log("constructed:", ts instanceof TransformStream);
console.log("ws hwm:", ws.highWaterMark);
console.log("rs hwm:", rs.highWaterMark);
