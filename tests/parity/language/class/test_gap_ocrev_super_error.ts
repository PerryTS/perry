class E extends Error{toString(){return super.toString()}} console.log(new E("x").toString());
