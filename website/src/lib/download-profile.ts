import type { ReleaseVariant } from "@/lib/releases";

/**
 * O que cada versão exige e para quem ela é. Descreve o produto, não o arquivo,
 * então mora aqui e não no snapshot da release (que o sync preserva como
 * estava e que não se edita à mão).
 *
 * Os requisitos são os do build, não um conselho: o whisper.cpp é compilado com
 * AVX2 fixo, e sem ele o app fecha na primeira transcrição; a versão CUDA desta
 * release só traz código para a geração RTX 40 (que as RTX 50 reaproveitam).
 */
export type VariantProfile = {
  title: string;
  short: string;
  audience: string;
  /** `caution` marca o que a versão NÃO cobre: leva alerta, não um ✓. */
  requirements: { text: string; caution?: boolean }[];
  model: string;
};

export const variantProfile: Record<ReleaseVariant, VariantProfile> = {
  cpu: {
    title: "ISPer para CPU",
    short: "CPU",
    audience: "Para qualquer PC com Windows. A escolha segura se você não tem uma placa NVIDIA RTX 40 ou mais nova.",
    requirements: [
      { text: "Windows 10 ou 11, 64 bits" },
      { text: "Processador com AVX2: Intel Core desde 2013 ou AMD desde 2015 (alguns Pentium e Celeron não têm)" },
      { text: "8 GB de memória ou mais" },
    ],
    model: "Modelo recomendado: Small, cerca de 490 MB, baixado na primeira configuração.",
  },
  cuda: {
    title: "ISPer para GPU NVIDIA (CUDA)",
    short: "CUDA",
    audience: "Para quem tem uma placa NVIDIA RTX 40 ou mais nova. Usa a placa para rodar os modelos grandes em tempo real.",
    requirements: [
      { text: "Windows 10 ou 11, 64 bits" },
      { text: "Placa NVIDIA GeForce RTX 40 ou RTX 50 (ou RTX Ada/PRO), com driver atualizado" },
      { text: "RTX 20 e RTX 30 ainda não rodam esta versão: nelas, use a versão CPU", caution: true },
    ],
    model: "Modelo recomendado: Large v3 Turbo, cerca de 575 MB, baixado na primeira configuração.",
  },
};
