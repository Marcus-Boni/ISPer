import assert from "node:assert/strict";
import test from "node:test";

import { parseArgs } from "./sync-args.mjs";

test("sem argumentos: release mais recente, gravando", () => {
  assert.deepEqual(parseArgs([]), { check: false, tag: null });
});

test("--tag com espaço, a forma que o workflow usa", () => {
  // portal-release-sync.yml: node scripts/sync-release.mjs --tag "$TAG".
  // Era exatamente isto que quebrava: o valor era lido e depois reprocessado
  // como argumento desconhecido.
  assert.deepEqual(parseArgs(["--tag", "v0.18.0"]), { check: false, tag: "v0.18.0" });
});

test("--tag= continua funcionando", () => {
  assert.deepEqual(parseArgs(["--tag=v0.18.0"]), { check: false, tag: "v0.18.0" });
});

test("--tag e --check juntos, em qualquer ordem", () => {
  const esperado = { check: true, tag: "v1.2.3" };
  assert.deepEqual(parseArgs(["--tag", "v1.2.3", "--check"]), esperado);
  assert.deepEqual(parseArgs(["--check", "--tag", "v1.2.3"]), esperado);
});

test("--tag sem valor é erro claro, não tag nula em silêncio", () => {
  // Antes, `--tag` no fim virava tag null e o script seguia com a release
  // mais recente — que nem sempre é a pedida.
  assert.throws(() => parseArgs(["--tag"]), /precisa de um valor/);
  assert.throws(() => parseArgs(["--tag", "--check"]), /precisa de um valor/);
});

test("tag fora do formato é recusada", () => {
  assert.throws(() => parseArgs(["--tag", "0.18.0"]), /tag de versão/);
  assert.throws(() => parseArgs(["--tag=latest"]), /tag de versão/);
});

test("argumento desconhecido continua sendo erro", () => {
  assert.throws(() => parseArgs(["--write"]), /argumento desconhecido: --write/);
});
