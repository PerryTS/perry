// HWS ipc: {"payload":42}
import {endianness} from "node:os";
console.log(endianness().length);
const env:any=process.env;
console.log(typeof env.NODE_CHANNEL_FD, typeof env.NODE_CHANNEL_SERIALIZATION_MODE);
const key=["NODE","CHANNEL","FD"].join("_");env[key]="user";
console.log(env[key]);const again:any=process.env;console.log(again[key]);console.log(process.env.NODE_CHANNEL_FD);
console.log(process.argv[2]);setTimeout(()=>console.log("timer"),1);
