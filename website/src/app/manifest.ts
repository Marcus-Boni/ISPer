import type { MetadataRoute } from "next";

export const dynamic = "force-static";

/**
 * Os ícones saem de `scripts/brand.py`, como todos os outros do projeto. O
 * maskable é uma placa à parte: o sistema o recorta em círculo ou squircle e só
 * garante os 80% centrais, então o fundo vai até a borda e a marca encolhe para
 * dentro da zona segura — usar a placa arredondada ali deixaria o recorte
 * mostrando os cantos.
 */
export default function manifest(): MetadataRoute.Manifest {
  return {
    name: "ISPer — Transcrição local com IA",
    short_name: "ISPer",
    description: "Portal oficial e documentação do ISPer.",
    start_url: "/",
    display: "standalone",
    background_color: "#161311",
    theme_color: "#f07e72",
    lang: "pt-BR",
    icons: [
      { src: "/icons/icon-192.png", sizes: "192x192", type: "image/png", purpose: "any" },
      { src: "/icons/icon-512.png", sizes: "512x512", type: "image/png", purpose: "any" },
      { src: "/icons/icon-maskable-512.png", sizes: "512x512", type: "image/png", purpose: "maskable" },
    ],
  };
}
