import * as https from "node:https";

function check(input: any) {
  try {
    const request = https.request(input);
    console.log("accepted");
    request.on("error", () => {});
    request.destroy();
  } catch (error: any) {
    console.log(error.name, error.code);
  }
}

check("http://127.0.0.1/");
check({ protocol: "http:", host: "127.0.0.1" });
