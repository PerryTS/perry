import Über, { Étage, Étage as Bâtiment, $Étage, Ü, grüße, salüt as привет, $, u__24_, λ, $val, _val } from "./unicode-symbols/étage.ts";
import * as namen from "./unicode-symbols/étage.ts";

class Ω {
  value: string;
  constructor(value = "café") { this.value = value; }
  méthode() { return this.value; }
  get größe() { return this.value.length; }
  static früh() { return "früh"; }
  "ré.sumé"() { return "point"; }
}
class Deuxième extends Ω {}
class Troisième extends Étage {}
function \u00E9cho(value: string) { return value; }
function $local() { return 47; }
function _local() { return 53; }
function add(a: number, b: number) { return a + b; }
function add$generic(a: number, b: number) { return a * b; }
const 闭包 = (value: number) => value + 1;

const local = new Ω();
console.log(local.méthode(), local.größe, Ω.früh());
console.log(local["ré.sumé"](), new Deuxième("deux").méthode(), new Troisième("trois").méthode());
console.log(écho("échappé"), $local(), _local(), 闭包(59));
console.log(add(2, 3), add$generic(2, 3));
const étage = new Étage();
console.log(étage.méthode(), étage.résumé, Étage.déjà(), Étage.Ü);
étage.résumé = "changé";
console.log(étage.méthode(), étage.résumé);
console.log(new Étage("étage").méthode());
console.log(new Bâtiment("alias").méthode(), привет("ami"));
console.log(new $Étage().méthode(), $Étage.name);
console.log(new Über().schön(), new Über("名").schön());
console.log(Ü, grüße(), $(), u__24_(), λ(), $val, _val);
console.log(namen.Ü, namen.grüße("monde"), namen.$(), namen.u__24_());
console.log(Ω.name, Étage.name, Über.name, grüße.name, λ.name, $.name, u__24_.name);
console.log(grüße.toString().includes("grüße"));
