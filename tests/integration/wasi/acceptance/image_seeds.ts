import { collect } from "perry/gc";
import { peer } from "./_helpers/image_seed_peer";

// Both compilation units emit the shared atom. Keep both literal families
// alive through synchronous moving collections and verify their key lists.
const local = { wrt2cSharedKey: 23, localOnly: "local" };
const remote = peer();
for (let round = 0; round < 8; round++) {
  const trash: any[] = [];
  for (let i = 0; i < 2000; i++) trash.push({ i, text: "seed-" + i });
  collect();
}
console.log(local.wrt2cSharedKey + remote.wrt2cSharedKey);
console.log(Object.keys(local).join(","));
console.log(Object.keys(remote).join(","));
console.log(local.localOnly, remote.peerOnly);
