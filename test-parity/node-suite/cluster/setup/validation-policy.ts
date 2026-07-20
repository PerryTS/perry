// Node freezes and validates schedulingPolicy on the first setupPrimary call.
import cluster from "node:cluster";

cluster.schedulingPolicy = 99 as any;
try {
  cluster.setupPrimary();
  console.log("accepted");
} catch (error: any) {
  console.log(error.name, error.code);
}
