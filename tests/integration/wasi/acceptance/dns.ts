import * as dns from "node:dns";
dns.lookup("localhost", { family: 4 }, (error, address, family) => {
  if (error) throw error;
  console.log(address, family);
});
