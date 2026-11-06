const metadata: any = { WebAssembly: { code: 7 } };
console.log("property:", metadata.WebAssembly.code);
console.log("computed:", metadata["WebAssembly"].code);
console.log("literal:", "WebAssembly");

function read(WebAssembly: any) {
  return WebAssembly.code;
}
console.log("shadowed:", read(metadata.WebAssembly));
