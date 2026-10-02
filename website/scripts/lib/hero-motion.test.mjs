import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

/**
 * O cursor do título ditado ficava preso na tela.
 *
 * `cursor-off` tinha só `to { opacity: 0 }`. O quadro inicial implícito herda o
 * valor de baixo na pilha, que é o 1 que o `cursor-on` segura, e com
 * `steps(1, end)` a saída só vira o quadro final com progresso exatamente 1.
 * No navegador de verdade, uma animação de 1 ms termina com progresso
 * 0,99999999999989 (é ponto flutuante), o degrau não acontece e fica o 1:
 * quatro dos cinco cursores continuavam visíveis depois do título pronto.
 *
 * Posicionar as animações em instantes redondos dava progresso 1 exato e
 * escondia o defeito, então o teste olha a própria folha de estilos: toda
 * animação finita em degraus que fica preenchida no fim declara os dois
 * extremos, para nunca depender do quadro implícito.
 */

const css = readFileSync(new URL("../../src/app/globals.css", import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");

function keyframes(name) {
  const start = css.search(new RegExp(`@keyframes\\s+${name}\\s*\\{`));
  if (start < 0) return null;
  let depth = 0;
  for (let i = css.indexOf("{", start); i < css.length; i++) {
    if (css[i] === "{") depth++;
    else if (css[i] === "}" && --depth === 0) return css.slice(css.indexOf("{", start) + 1, i);
  }
  return null;
}

/** Seletores e declarações de cada quadro: `from, 50%` → ["from", "50%"]. */
function frames(body) {
  return [...body.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map(([, selectors, decls]) => ({
    stops: selectors.split(",").map((s) => s.trim()),
    decls: decls.trim(),
  }));
}

/** Os itens de cada `animation:`, separados por vírgula fora de parênteses. */
function animationItems() {
  const items = [];
  for (const [, value] of css.matchAll(/(?:^|[;{\s])animation\s*:\s*([^;}]+)/g)) {
    let depth = 0;
    let current = "";
    for (const ch of value) {
      if (ch === "(") depth++;
      if (ch === ")") depth--;
      if (ch === "," && depth === 0) {
        items.push(current.trim());
        current = "";
      } else current += ch;
    }
    items.push(current.trim());
  }
  return items;
}

test("cursor-off esconde o cursor nos dois extremos, qualquer que seja o progresso final", () => {
  const body = keyframes("cursor-off");
  assert.ok(body, "@keyframes cursor-off existe");
  const stops = frames(body);
  const at = (stop) => stops.find((f) => f.stops.includes(stop));
  for (const [stop, alias] of [["from", "0%"], ["to", "100%"]]) {
    const frame = at(stop) ?? at(alias);
    assert.ok(frame, `cursor-off declara ${stop}`);
    assert.match(frame.decls, /opacity\s*:\s*0\s*(;|$)/, `cursor-off ${stop} tem opacity: 0`);
  }
});

test("toda animação finita em degraus e preenchida no fim declara o início e o fim", () => {
  const stepped = animationItems().filter(
    (item) => /steps\(/.test(item) && /\b(forwards|both)\b/.test(item) && !/\binfinite\b/.test(item),
  );
  assert.ok(stepped.length > 0, "a folha tem animações em degraus para conferir");
  for (const item of stepped) {
    const name = item.split(/\s+/).find((token) => keyframes(token) !== null);
    assert.ok(name, `os keyframes de "${item}" existem`);
    const stops = frames(keyframes(name)).flatMap((f) => f.stops);
    assert.ok(stops.includes("from") || stops.includes("0%"), `${name} declara o quadro inicial`);
    assert.ok(stops.includes("to") || stops.includes("100%"), `${name} declara o quadro final`);
  }
});
