import { connect } from "node:http2";

for (const protocol of ["ftp:", "file:"]) {
  try {
    connect(`${protocol}//localhost`);
  } catch (error: any) {
    console.log(protocol, error.name, error.code);
  }
}
