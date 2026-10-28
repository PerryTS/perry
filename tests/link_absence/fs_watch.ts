// HWS companion: fs
// HWS hook: js_nm_install_fs
// HWS prefixes: perry_runtime::fs::|js_fs_
import {watch,writeFileSync,unlinkSync} from "node:fs";
const path="hws2-watch.txt";writeFileSync(path,"first");
let seen=false;let watcher:any;let deadline:any;
watcher=watch(path,()=>{
    if(seen)return;seen=true;watcher.close();clearTimeout(deadline);
    unlinkSync(path);console.log("changed");
});
deadline=setTimeout(()=>{watcher.close();unlinkSync(path);throw new Error("watch event missing");},3000);
setTimeout(()=>writeFileSync(path,"second"),100);
