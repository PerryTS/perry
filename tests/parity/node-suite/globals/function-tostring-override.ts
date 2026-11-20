function show(label: string, value: unknown) {
  console.log(label + ":", JSON.stringify(value));
}

function plain() {
  return 1;
}

const arrow = () => 2;

Function.prototype.toString = function () {
  if (this === plain) {
    return "shifted:plain";
  }
  if (this === arrow) {
    return "shifted:arrow";
  }
  return "shifted:" + typeof this;
};

show("String plain", String(plain));
show("plain toString", plain.toString());
show("String arrow", String(arrow));
show("boxed String plain", String(new String(plain)));
