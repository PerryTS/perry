const body = '{"title":"t","permission":[],"zeta":1,"10":"ten","2":"two"}';

const request = await new Request("http://example.com/", {
  method: "POST",
  body,
}).json();
console.log("request json", JSON.stringify(request));
console.log("request keys", JSON.stringify(Object.keys(request)));

const response = await new Response(body).json();
console.log("response json", JSON.stringify(response));
console.log("response keys", JSON.stringify(Object.keys(response)));
