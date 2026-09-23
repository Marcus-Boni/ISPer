/**
 * O que cada arquivo da release é, pelo nome. O release.yml dá nome a tudo:
 *
 *   ISPer_<v>_x64-cpu-setup.exe      instalador CPU
 *   ISPer_<v>_x64-setup.exe          instalador CUDA
 *   ISPer_<v>_x64-cpu-portable.zip   versão portátil CPU (desde a 0.20.0)
 *   ISPer_<v>_x64-portable.zip       versão portátil CUDA (desde a 0.20.0)
 *
 * O resto (assinaturas .sig, latest*.json, SBOM, SHA256SUMS.txt) não vai para
 * a página de download e devolve null.
 */
export function classifyAsset(name) {
  if (/-cpu-setup\.exe$/i.test(name)) return { variant: "cpu", kind: "installer" };
  if (/-setup\.exe$/i.test(name)) return { variant: "cuda", kind: "installer" };
  if (/-cpu-portable\.zip$/i.test(name)) return { variant: "cpu", kind: "portable" };
  if (/-portable\.zip$/i.test(name)) return { variant: "cuda", kind: "portable" };
  return null;
}

const variantsOf = (assets, kind) =>
  assets
    .filter((asset) => asset.kind === kind)
    .map((asset) => asset.variant)
    .sort()
    .join(",");

/**
 * A página existe para o leitor escolher entre CPU e CUDA: um instalador
 * sozinho é erro de empacotamento, não snapshot válido. Os zips portáteis são
 * opcionais (as releases até a 0.19.0 não têm), mas, quando existem, vêm os
 * dois — um só é o mesmo erro.
 */
export function checkAssetSet(assets) {
  const installers = variantsOf(assets, "installer");
  if (installers !== "cpu,cuda") {
    throw new Error(`esperava instaladores cpu e cuda, encontrei: ${installers || "nenhum"}`);
  }
  const portables = variantsOf(assets, "portable");
  if (portables !== "" && portables !== "cpu,cuda") {
    throw new Error(`esperava os zips portáteis cpu e cuda (ou nenhum), encontrei: ${portables}`);
  }
}

const KIND_ORDER = { installer: 0, portable: 1, source: 2 };

/** Instaladores primeiro, depois os portáteis; CPU antes de CUDA em cada grupo. */
export function sortAssets(assets) {
  return [...assets].sort(
    (a, b) => KIND_ORDER[a.kind] - KIND_ORDER[b.kind] || a.variant.localeCompare(b.variant),
  );
}

/** A chave dos campos editoriais: o mesmo hardware pode ter instalador e zip. */
export function editorialKey(asset) {
  return `${asset.kind ?? "installer"}:${asset.variant}`;
}
