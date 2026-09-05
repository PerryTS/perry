// Genuine dead-zone reads must name the source binding, including captures.
function captures(): void {
  const read = () => later();
  try { read(); } catch (error) { console.log(error.name, error.message); }
  const later = () => "initialized";
  console.log(read(), later());
}

function localAndCaptured(): void {
  const read = () => value;
  try { console.log(value); } catch (error) { console.log(error.name, error.message); }
  try { read(); } catch (error) { console.log(error.name, error.message); }
  let value = 41;
  value++;
  console.log(read());
}

function typeOfAndUpdate(): void {
  const type = () => typeof count;
  const update = () => count++;
  try { type(); } catch (error) { console.log(error.name, error.message); }
  try { update(); } catch (error) { console.log(error.name, error.message); }
  let count = 10;
  console.log(type(), update(), count);
}

function nestedNames(): void {
  const value = "outer";
  {
    const read = () => value;
    try { read(); } catch (error) { console.log(error.name, error.message); }
    const value = "inner";
    console.log(read());
  }
  console.log(value);
  const readUnicode = () => café;
  try { readUnicode(); } catch (error) { console.log(error.name, error.message); }
  const café = "ready";
  console.log(readUnicode());
}

captures();
localAndCaptured();
typeOfAndUpdate();
nestedNames();
