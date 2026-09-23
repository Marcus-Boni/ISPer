import assert from "node:assert/strict";
import test from "node:test";

import { checkAssetSet, classifyAsset, editorialKey, sortAssets } from "./release-assets.mjs";

test("os quatro arquivos que a página mostra", () => {
  assert.deepEqual(classifyAsset("ISPer_0.20.0_x64-cpu-setup.exe"), { variant: "cpu", kind: "installer" });
  assert.deepEqual(classifyAsset("ISPer_0.20.0_x64-setup.exe"), { variant: "cuda", kind: "installer" });
  assert.deepEqual(classifyAsset("ISPer_0.20.0_x64-cpu-portable.zip"), { variant: "cpu", kind: "portable" });
  assert.deepEqual(classifyAsset("ISPer_0.20.0_x64-portable.zip"), { variant: "cuda", kind: "portable" });
});

test("o resto da release fica de fora", () => {
  // Os nomes reais da v0.19.0, fora os dois instaladores.
  for (const name of [
    "ISPer_0.19.0_sbom.cdx.json",
    "ISPer_0.19.0_x64-cpu-setup.exe.sig",
    "ISPer_0.19.0_x64-setup.exe.sig",
    "latest-cpu.json",
    "latest.json",
    "SHA256SUMS.txt",
  ]) {
    assert.equal(classifyAsset(name), null, name);
  }
});

const asset = (kind, variant) => ({ kind, variant });

test("release até a 0.19.0: só os dois instaladores é válido", () => {
  assert.doesNotThrow(() => checkAssetSet([asset("installer", "cuda"), asset("installer", "cpu")]));
});

test("release com os dois zips portáteis é válida", () => {
  assert.doesNotThrow(() =>
    checkAssetSet([
      asset("installer", "cpu"),
      asset("installer", "cuda"),
      asset("portable", "cpu"),
      asset("portable", "cuda"),
    ]),
  );
});

test("um instalador ou um zip sozinho é erro de empacotamento", () => {
  assert.throws(() => checkAssetSet([asset("installer", "cpu")]), /instaladores cpu e cuda/);
  assert.throws(() => checkAssetSet([]), /encontrei: nenhum/);
  assert.throws(
    () => checkAssetSet([asset("installer", "cpu"), asset("installer", "cuda"), asset("portable", "cpu")]),
    /zips portáteis cpu e cuda/,
  );
});

test("ordem: instaladores e depois portáteis, CPU antes de CUDA", () => {
  const sorted = sortAssets([
    asset("portable", "cuda"),
    asset("installer", "cuda"),
    asset("portable", "cpu"),
    asset("installer", "cpu"),
  ]);
  assert.deepEqual(
    sorted.map((a) => `${a.kind}:${a.variant}`),
    ["installer:cpu", "installer:cuda", "portable:cpu", "portable:cuda"],
  );
});

test("a chave editorial separa instalador e zip do mesmo hardware", () => {
  assert.equal(editorialKey(asset("portable", "cpu")), "portable:cpu");
  assert.equal(editorialKey({ variant: "cuda" }), "installer:cuda");
});
