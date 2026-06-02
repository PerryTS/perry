import tty from "node:tty";

try { const rs: any = new tty.ReadStream(0); console.log("readstream:", typeof rs.isRaw, typeof rs.setRawMode); } catch (err: any) { console.log("readstream:", err?.name, err?.code || "no-code"); }
try { const ws: any = new tty.WriteStream(1); console.log("writestream:", typeof ws.columns, typeof ws.rows, typeof ws.getColorDepth); } catch (err: any) { console.log("writestream:", err?.name, err?.code || "no-code"); }
try { new tty.ReadStream("0" as any); console.log("bad readstream no throw"); } catch (err: any) { console.log("bad readstream:", err?.name, err?.code || "no-code"); }
