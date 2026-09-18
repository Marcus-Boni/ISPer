import type { MetadataRoute } from "next";

export const dynamic = "force-static";

export default function manifest(): MetadataRoute.Manifest {
  return { name: "ISPer — Transcrição local com IA", short_name: "ISPer", description: "Portal oficial e documentação do ISPer.", start_url: "/", display: "standalone", background_color: "#161311", theme_color: "#f07e72", lang: "pt-BR" };
}
