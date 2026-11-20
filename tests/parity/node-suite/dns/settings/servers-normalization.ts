import dns from "node:dns";

for (
  const servers of [
    ["4.4.4.4:53", "[2001:4860:4860::8888]:53"],
    ["103.238.225.181:666", "[fe80::483a:5aff:fee6:1f04]:666"],
    ["fe80::483a:5aff:fee6:1f04", "[fe80::483a:5aff:fee6:1f04]"],
    [],
  ]
) {
  dns.setServers(servers);
  console.log(JSON.stringify(servers) + ":", dns.getServers().join("|"));
}
