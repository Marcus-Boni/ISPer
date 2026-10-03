"use client";

import { useSyncExternalStore } from "react";
import { Cpu, Download, MonitorCog } from "lucide-react";
import { classifyGpu, recommendVariant } from "@/lib/download-guide.mjs";
import { variantProfile } from "@/lib/download-profile";
import type { ReleaseAsset, ReleaseVariant } from "@/lib/releases";
import { formatBytes } from "@/lib/releases";

type Reason = ReturnType<typeof recommendVariant>["reason"];
type Detected = { variant: ReleaseVariant; reason: Reason; gpuName: string | null };

/** O nome da placa que o navegador expõe. Vazio quando ele não diz. */
function readRenderer(): string {
  try {
    const canvas = document.createElement("canvas");
    const gl = (canvas.getContext("webgl") ?? canvas.getContext("experimental-webgl")) as WebGLRenderingContext | null;
    if (!gl) return "";
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    const renderer = info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
    gl.getExtension("WEBGL_lose_context")?.loseContext();
    return typeof renderer === "string" ? renderer : "";
  } catch {
    return "";
  }
}

function isWindows(): boolean {
  const data = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData;
  return /windows/i.test(data?.platform || navigator.userAgent);
}

/* A máquina não muda enquanto a página está aberta: detecta uma vez e guarda. */
let cached: Detected | null = null;
function detect(): Detected {
  if (!cached) {
    const gpu = classifyGpu(readRenderer());
    cached = { ...recommendVariant({ windows: isWindows(), gpu }), gpuName: gpu.name };
  }
  return cached;
}
const noSubscription = () => () => {};

/** Uma frase, a do motivo. Fala do computador da pessoa, não da regra. */
function reasonText(detected: Detected): string {
  const gpu = detected.gpuName;
  switch (detected.reason) {
    case "cuda-ready":
      return `Encontramos uma ${gpu ?? "placa NVIDIA RTX 40 ou mais nova"} neste computador: a versão CUDA usa a placa para transcrever mais rápido.`;
    case "nvidia-older":
      return `Encontramos uma ${gpu ?? "placa NVIDIA"}. A versão CUDA desta release só roda em placas RTX 40 ou mais novas, então a versão CPU é a que funciona aqui.`;
    case "no-nvidia":
      return `Este navegador usa ${gpu ? `a ${gpu}` : "uma placa que não é NVIDIA"}. Se o seu notebook também tem uma NVIDIA RTX 40 ou mais nova, a versão CUDA é a mais rápida.`;
    case "not-windows":
      return "O ISPer é para Windows 10 e 11. Você pode baixar daqui e abrir o instalador no PC em que vai usar.";
    default:
      return "Não conseguimos ver a placa de vídeo deste computador, então recomendamos a versão que roda em qualquer PC.";
  }
}

export function RecommendedDownload({ assets, version }: { assets: ReleaseAsset[]; version: string }) {
  // O servidor não sabe nada da máquina: a CPU é a recomendação que nunca
  // deixa ninguém com um app que fecha. O navegador refina depois.
  const detected = useSyncExternalStore(noSubscription, detect, () => null);

  const variant: ReleaseVariant = detected?.variant ?? "cpu";
  const main = assets.find((a) => a.variant === variant) ?? assets[0];
  const other = assets.find((a) => a.variant !== main?.variant);
  if (!main) return null;
  const profile = variantProfile[main.variant];

  return (
    <section className="recommend" aria-labelledby="recomendado">
      <div className="recommend-body">
        <span className="recommend-icon" aria-hidden="true">{main.variant === "cuda" ? <MonitorCog /> : <Cpu />}</span>
        <div>
          <h2 id="recomendado">{profile.title}</h2>
          <p className="recommend-audience">{profile.audience}</p>
        </div>
      </div>
      <div className="recommend-action">
        <a className="button button-primary button-large recommend-button" href={main.downloadUrl}>
          <Download aria-hidden="true" />
          Baixar o ISPer {version}
          <span className="button-tag">{formatBytes(main.sizeBytes)}</span>
        </a>
        {/* Numa NVIDIA mais antiga, a outra versão é justamente a que fecharia:
            ela continua na comparação, com o aviso, mas não ao lado do botão. */}
        {other && detected?.reason !== "nvidia-older" ? (
          <a className="text-link recommend-other" href={other.downloadUrl}>
            ou baixar a versão {variantProfile[other.variant].short} ({formatBytes(other.sizeBytes)})
          </a>
        ) : null}
      </div>
      <p className="recommend-reason" aria-live="polite">
        {detected ? reasonText(detected) : "Funciona em qualquer PC com Windows 10 ou 11 e processador com AVX2."}
      </p>
    </section>
  );
}
