/**
 * Qual versão do ISPer recomendar para o computador de quem abre a página.
 *
 * O navegador expõe o nome da placa de vídeo pelo WebGL. É um palpite, não um
 * inventário: num notebook com duas placas o navegador pode estar na integrada,
 * e alguns navegadores mascaram o nome. Por isso a regra erra para o lado que
 * sempre funciona: na dúvida, a versão CPU.
 *
 * Até a 0.25.0, a versão CUDA era compilada só para a arquitetura Ada (RTX 40),
 * cujo código as RTX 50 reaproveitam; RTX 20 e 30 fechavam na primeira
 * transcrição. Depois dela, o release.yml compila também Turing e Ampere, e a
 * lista ampliada vale — `wideCuda` diz qual das duas listas usar.
 */

/** Placas em que a versão CUDA da 0.25.0 e anteriores funciona. */
const CUDA_READY = [
  /\bRTX\s*(?:40|50)\d{2}\b/i, // GeForce RTX 40 e 50, inclusive Laptop GPU
  /\bRTX\s*\d{4}\s*Ada\b/i, // RTX 2000/4000/5000/6000 Ada
  /\bRTX\s*PRO\s*\d{3,4}\b/i, // RTX PRO (Blackwell)
  /\bBlackwell\b/i,
];

/** Desde as releases compiladas para Turing (7.5), Ampere (8.6), Ada (8.9) e Blackwell. */
const CUDA_READY_WIDE = [
  ...CUDA_READY,
  /\bRTX\s*(?:20|30)\d{2}\b/i, // GeForce RTX 20 e 30, inclusive a RTX 2050 (Ampere)
  /\bGTX\s*16\d{2}\b/i, // GTX 1650/1660: Turing
  /\bTITAN\s*RTX\b/i,
  /\bQuadro\s*RTX\b/i, // Quadro RTX: Turing
  /\bRTX\s*A\d{3,4}\b/i, // RTX A2000…A6000: Ampere
];

/**
 * Lê o texto que o WebGL devolve, como
 * "ANGLE (NVIDIA, NVIDIA GeForce RTX 4050 Laptop GPU (0x000028A1) Direct3D11 vs_5_0 ps_5_0, D3D11)".
 * @param {string | null | undefined} renderer
 * @param {{ wideCuda?: boolean }} [options]
 * @returns {{ vendor: "nvidia" | "amd" | "intel" | "unknown", name: string | null, cudaReady: boolean }}
 */
export function classifyGpu(renderer, { wideCuda = false } = {}) {
  const raw = (renderer ?? "").trim();
  if (!raw || /SwiftShader|llvmpipe|Microsoft Basic Render|Software/i.test(raw)) {
    return { vendor: "unknown", name: null, cudaReady: false };
  }
  const name = gpuName(raw);
  if (/NVIDIA|GeForce|Quadro|\bRTX\b|\bGTX\b/i.test(raw)) {
    const ready = wideCuda ? CUDA_READY_WIDE : CUDA_READY;
    return { vendor: "nvidia", name, cudaReady: ready.some((re) => re.test(raw)) };
  }
  if (/\bAMD\b|Radeon|ATI Technologies/i.test(raw)) return { vendor: "amd", name, cudaReady: false };
  if (/Intel/i.test(raw)) return { vendor: "intel", name, cudaReady: false };
  return { vendor: "unknown", name: null, cudaReady: false };
}

/** O nome da placa, sem o envelope do ANGLE nem o id do dispositivo. */
function gpuName(raw) {
  const angle = raw.match(/^ANGLE \(([^,]+),\s*(.+?)(?:\s*\(0x[0-9a-f]+\))?\s*(?:Direct3D|OpenGL|Vulkan|Metal|,)/i);
  const name = (angle ? angle[2] : raw)
    .replace(/\s*\(0x[0-9a-f]+\)/gi, "")
    .replace(/\s*(?:Direct3D|D3D)\d*.*$/i, "")
    .replace(/\s*\/PCIe\/SSE2$/i, "")
    .trim();
  return name || null;
}

/**
 * @param {{ windows: boolean, gpu: ReturnType<typeof classifyGpu> }} machine
 * @returns {{ variant: "cpu" | "cuda", reason: "cuda-ready" | "nvidia-older" | "no-nvidia" | "unknown" | "not-windows" }}
 */
export function recommendVariant({ windows, gpu }) {
  if (!windows) return { variant: "cpu", reason: "not-windows" };
  if (gpu.vendor === "nvidia") {
    return gpu.cudaReady ? { variant: "cuda", reason: "cuda-ready" } : { variant: "cpu", reason: "nvidia-older" };
  }
  if (gpu.vendor === "unknown") return { variant: "cpu", reason: "unknown" };
  return { variant: "cpu", reason: "no-nvidia" };
}

/**
 * A partir de qual versão a CUDA roda em RTX 20 e 30. A página segue a versão
 * publicada no snapshot: a promessa só aparece quando o instalador que a cumpre
 * está no ar. As releases saem da main, em linha, então toda versão depois da
 * 0.25.0 já é compilada com as gerações novas.
 * @param {string} version
 */
export function hasWideCuda(version) {
  const [major = 0, minor = 0, patch = 0] = String(version).split(".").map((part) => Number.parseInt(part, 10) || 0);
  return major > 0 || minor > 25 || (minor === 25 && patch > 0);
}
