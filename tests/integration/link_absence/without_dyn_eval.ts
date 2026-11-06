class Box{};const box:any=new Box();(Box.prototype as any).value=42;console.log(box.value);delete (Box.prototype as any).value;console.log(typeof box.value);
try { throw new Error("ordinary"); } catch (error) { console.log(error.message); }
