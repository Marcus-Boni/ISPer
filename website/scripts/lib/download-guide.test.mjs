import assert from "node:assert/strict";
import test from "node:test";

import { classifyGpu, hasWideCuda, recommendVariant } from "../../src/lib/download-guide.mjs";

// Textos reais que o WebGL devolve no Chrome/Edge (ANGLE) e no Firefox.
const RTX4050 = "ANGLE (NVIDIA, NVIDIA GeForce RTX 4050 Laptop GPU (0x000028A1) Direct3D11 vs_5_0 ps_5_0, D3D11)";
const RTX3060 = "ANGLE (NVIDIA, NVIDIA GeForce RTX 3060 (0x00002503) Direct3D11 vs_5_0 ps_5_0, D3D11)";
const RTX5070 = "ANGLE (NVIDIA, NVIDIA GeForce RTX 5070 (0x00002F04) Direct3D11 vs_5_0 ps_5_0, D3D11)";
const GTX1650 = "NVIDIA GeForce GTX 1650/PCIe/SSE2";
const ADA = "ANGLE (NVIDIA, NVIDIA RTX 2000 Ada Generation Laptop GPU (0x000028B8) Direct3D11 vs_5_0 ps_5_0, D3D11)";
const INTEL = "ANGLE (Intel, Intel(R) UHD Graphics 620 (0x00005917) Direct3D11 vs_5_0 ps_5_0, D3D11)";
const AMD = "ANGLE (AMD, AMD Radeon RX 6600 (0x000073FF) Direct3D11 vs_5_0 ps_5_0, D3D11)";
const SOFTWARE = "ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)";

test("RTX 40, RTX 50 e Ada rodam a versão CUDA desta release", () => {
  for (const r of [RTX4050, RTX5070, ADA]) {
    const gpu = classifyGpu(r);
    assert.equal(gpu.vendor, "nvidia", r);
    assert.equal(gpu.cudaReady, true, r);
    assert.deepEqual(recommendVariant({ windows: true, gpu }), { variant: "cuda", reason: "cuda-ready" });
  }
});

test("NVIDIA mais antiga recebe a versão CPU, com o motivo", () => {
  for (const r of [RTX3060, GTX1650]) {
    const gpu = classifyGpu(r);
    assert.equal(gpu.vendor, "nvidia", r);
    assert.equal(gpu.cudaReady, false, r);
    assert.deepEqual(recommendVariant({ windows: true, gpu }), { variant: "cpu", reason: "nvidia-older" });
  }
});

test("o nome da placa sai sem o envelope do ANGLE", () => {
  assert.equal(classifyGpu(RTX4050).name, "NVIDIA GeForce RTX 4050 Laptop GPU");
  assert.equal(classifyGpu(GTX1650).name, "NVIDIA GeForce GTX 1650");
  assert.equal(classifyGpu(INTEL).name, "Intel(R) UHD Graphics 620");
});

test("Intel, AMD, software ou nada: versão CPU", () => {
  assert.equal(recommendVariant({ windows: true, gpu: classifyGpu(INTEL) }).reason, "no-nvidia");
  assert.equal(recommendVariant({ windows: true, gpu: classifyGpu(AMD) }).reason, "no-nvidia");
  assert.equal(recommendVariant({ windows: true, gpu: classifyGpu(SOFTWARE) }).reason, "unknown");
  assert.equal(recommendVariant({ windows: true, gpu: classifyGpu("") }).reason, "unknown");
  assert.equal(recommendVariant({ windows: true, gpu: classifyGpu(null) }).variant, "cpu");
});

test("fora do Windows a recomendação é a CPU e o motivo diz por quê", () => {
  assert.deepEqual(recommendVariant({ windows: false, gpu: classifyGpu(RTX4050) }), { variant: "cpu", reason: "not-windows" });
});

const RTX2060 = "ANGLE (NVIDIA, NVIDIA GeForce RTX 2060 (0x00001F08) Direct3D11 vs_5_0 ps_5_0, D3D11)";
const GTX1080 = "ANGLE (NVIDIA, NVIDIA GeForce GTX 1080 (0x00001B80) Direct3D11 vs_5_0 ps_5_0, D3D11)";
const RTXA2000 = "ANGLE (NVIDIA, NVIDIA RTX A2000 Laptop GPU (0x000025B8) Direct3D11 vs_5_0 ps_5_0, D3D11)";

test("com as gerações novas, RTX 20, RTX 30, GTX 16 e RTX A também rodam a CUDA", () => {
  for (const r of [RTX2060, RTX3060, GTX1650, RTXA2000, RTX4050, RTX5070]) {
    const gpu = classifyGpu(r, { wideCuda: true });
    assert.equal(gpu.cudaReady, true, r);
    assert.equal(recommendVariant({ windows: true, gpu }).variant, "cuda", r);
  }
});

test("GTX 10 e anteriores continuam na CPU mesmo com as gerações novas", () => {
  const gpu = classifyGpu(GTX1080, { wideCuda: true });
  assert.equal(gpu.cudaReady, false);
  assert.deepEqual(recommendVariant({ windows: true, gpu }), { variant: "cpu", reason: "nvidia-older" });
});

test("a promessa ampliada só vale para versões depois da 0.25.0", () => {
  assert.equal(hasWideCuda("0.25.0"), false);
  assert.equal(hasWideCuda("0.24.0"), false);
  assert.equal(hasWideCuda("0.25.1"), true);
  assert.equal(hasWideCuda("0.26.0"), true);
  assert.equal(hasWideCuda("1.0.0"), true);
});
