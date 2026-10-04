// #11826 fixture: runs first in the cycle and reaches into cyc_a early.
import { CA, LA, readCA, readLA } from "./cyc_a.ts";
import * as nsA from "./cyc_a.ts";
function name(e: any): string { return e && e.constructor ? e.constructor.name : String(e); }
try { readCA(); console.log("cyc fn no throw"); } catch (e) { console.log("cyc-fn", name(e)); }
try { readLA(); console.log("cyc fn2 no throw"); } catch (e) { console.log("cyc-fn-let", name(e)); }
try { console.log(CA.v); } catch (e) { console.log("cyc-import-read", name(e)); }
try { console.log(LA); } catch (e) { console.log("cyc-import-let", name(e)); }
const viaNs: any = nsA;
try { console.log(viaNs.LA); } catch (e) { console.log("cyc-namespace-let", name(e)); }
export const fromB = "b";
