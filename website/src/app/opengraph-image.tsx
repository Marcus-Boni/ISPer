import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { ImageResponse } from "next/og";
import { BRAND_MARK } from "@/lib/brand-mark";

export const alt = "ISPer — transcrição local com IA para Windows";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export const dynamic = "force-static";

/**
 * As fontes do site, em TTF estático. O Satori não lê WOFF2 nem eixos
 * variáveis, então scripts/og-fonts.py gera, das fontes do próprio site, uma
 * instância por espessura e tamanho óptico usados aqui — sem elas a imagem saía
 * numa serifa genérica.
 */
const font = (file: string) => readFile(join(process.cwd(), "src/assets/fonts/og", file));
const [brand, title, titleItalic, text] = await Promise.all([
  font("fraunces-720-opsz42.ttf"),
  font("fraunces-760-opsz78.ttf"),
  font("fraunces-520-opsz78.ttf"),
  font("hanken-grotesk-400.ttf"),
]);

/**
 * O Satori só registra espessuras em centenas. Cada uma é aqui só a chave que
 * escolhe a instância: o desenho é o da espessura do site, gravado no arquivo.
 */
const WEIGHT = { brand: 700, title: 800, titleItalic: 500, text: 400 } as const;

/**
 * A Fraunces do site é só o romano: o itálico de "No seu computador." é o
 * Chrome inclinando o romano, com cisalhamento de 1/4. O Satori não inclina
 * fonte nenhuma, então a linha leva a mesma inclinação, presa na base.
 */
const SYNTHETIC_ITALIC = `skewX(${(-Math.atan(0.25) * 180) / Math.PI}deg)`;

export default function OpenGraphImage() {
  return new ImageResponse(
    <div style={{ width: "100%", height: "100%", display: "flex", flexDirection: "column", justifyContent: "space-between", padding: "70px 78px", background: "#161311", color: "#ece7e1", fontFamily: "Fraunces", position: "relative" }}>
      <div style={{ position: "absolute", width: 520, height: 520, borderRadius: 520, background: "rgba(240,126,114,.14)", filter: "blur(80px)", top: -260, left: -100 }} />
      {/* A marca vem antes do nome, como em toda placa do ISPer. O ponto de
          gravação e o ponto final do "ISPer." rimam de propósito. */}
      <div style={{ display: "flex", alignItems: "center", gap: 22 }}>
        <svg width={56} height={48} viewBox={BRAND_MARK.viewBox}>
          {BRAND_MARK.bars.map((bar) => <rect key={bar.x} x={bar.x} y={bar.y} width={bar.width} height={bar.height} fill={BRAND_MARK.colors.bars} />)}
          <circle cx={BRAND_MARK.dot.cx} cy={BRAND_MARK.dot.cy} r={BRAND_MARK.dot.r} fill={BRAND_MARK.colors.dot} />
        </svg>
        {/* .brand do site: 720, -0.02em. */}
        <div style={{ display: "flex", fontSize: 42, fontWeight: WEIGHT.brand, letterSpacing: "-0.84px" }}>ISPer<span style={{ color: "#f07e72" }}>.</span></div>
      </div>
      <div style={{ display: "flex", flexDirection: "column", maxWidth: 980 }}>
        {/* .hero-copy h1 do site: 760 e -0.03em; o em, 520. */}
        <div style={{ display: "flex", flexDirection: "column", fontSize: 78, fontWeight: WEIGHT.title, lineHeight: 1.03, letterSpacing: "-2.34px" }}>
          <span>Suas palavras.</span>
          <span style={{ color: "#f79f94", fontWeight: WEIGHT.titleItalic, transform: SYNTHETIC_ITALIC, transformOrigin: "0 100%" }}>No seu computador.</span>
        </div>
        <div style={{ marginTop: 26, fontSize: 27, color: "#d3cbc3", fontFamily: "Hanken Grotesk" }}>Ditado e transcrição de reuniões com IA local para Windows.</div>
      </div>
      <div style={{ display: "flex", gap: 18, fontSize: 20, color: "#a79e96", fontFamily: "Hanken Grotesk" }}><span>Código aberto</span><span>·</span><span>CPU e CUDA</span><span>·</span><span>Sem mensalidade</span></div>
    </div>,
    {
      ...size,
      fonts: [
        { name: "Fraunces", data: brand, weight: WEIGHT.brand, style: "normal" },
        { name: "Fraunces", data: title, weight: WEIGHT.title, style: "normal" },
        { name: "Fraunces", data: titleItalic, weight: WEIGHT.titleItalic, style: "normal" },
        { name: "Hanken Grotesk", data: text, weight: WEIGHT.text, style: "normal" },
      ],
    },
  );
}
