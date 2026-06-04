class Base {
  value = "base";
}

function Tagged(BaseClass: any) {
  return class extends BaseClass {
    tag() {
      return "tag:" + this.value;
    }
  };
}

const Mixed = Tagged(Base);
console.log("classes/mixin-expression:" + new Mixed().tag());
