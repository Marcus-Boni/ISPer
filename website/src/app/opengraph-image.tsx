import { ImageResponse } from "next/og";
import { BRAND_MARK } from "@/lib/brand-mark";

export const alt = "ISPer — transcrição local com IA para Windows";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export const dynamic = "force-static";

export default function OpenGraphImage() {
  return new ImageResponse(
    <div style={{ width: "100%", height: "100%", display: "flex", flexDirection: "column", justifyContent: "space-between", padding: "70px 78px", background: "#161311", color: "#ece7e1", fontFamily: "serif", position: "relative" }}>
      <div style={{ position: "absolute", width: 520, height: 520, borderRadius: 520, background: "rgba(240,126,114,.14)", filter: "blur(80px)", top: -260, left: -100 }} />
      {/* A marca vem antes do nome, como em toda placa do ISPer. O ponto de
          gravação e o ponto final do "ISPer." rimam de propósito. */}
      <div style={{ display: "flex", alignItems: "center", gap: 22 }}>
        <svg width={56} height={48} viewBox={BRAND_MARK.viewBox}>
          {BRAND_MARK.bars.map((bar) => <rect key={bar.x} x={bar.x} y={bar.y} width={bar.width} height={bar.height} fill={BRAND_MARK.colors.bars} />)}
          <circle cx={BRAND_MARK.dot.cx} cy={BRAND_MARK.dot.cy} r={BRAND_MARK.dot.r} fill={BRAND_MARK.colors.dot} />
        </svg>
        <div style={{ display: "flex", fontSize: 42, fontWeight: 700, letterSpacing: "-1px" }}>ISPer<span style={{ color: "#f07e72" }}>.</span></div>
      </div>
      <div style={{ display: "flex", flexDirection: "column", maxWidth: 980 }}>
        <div style={{ display: "flex", flexDirection: "column", fontSize: 78, lineHeight: 1.03, letterSpacing: "-3px" }}><span>Suas palavras.</span><span style={{ color: "#f79f94", fontStyle: "italic" }}>No seu computador.</span></div>
        <div style={{ marginTop: 26, fontSize: 27, color: "#d3cbc3", fontFamily: "sans-serif" }}>Ditado e transcrição de reuniões com IA local para Windows.</div>
      </div>
      <div style={{ display: "flex", gap: 18, fontSize: 20, color: "#a79e96", fontFamily: "sans-serif" }}><span>Código aberto</span><span>·</span><span>CPU e CUDA</span><span>·</span><span>Sem mensalidade</span></div>
    </div>,
    size,
  );
}
