import type { MetadataRoute } from "next";
import { getAllDocs } from "@/lib/docs";
import { siteConfig } from "@/lib/site";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  const base = siteConfig.url;
  const docs = getAllDocs();
  const changed = new Date("2026-09-15T00:00:00-03:00");
  return [
    { url: base, lastModified: changed, changeFrequency: "weekly", priority: 1 },
    { url: `${base}/download/`, lastModified: changed, changeFrequency: "weekly", priority: .9 },
    { url: `${base}/docs/`, lastModified: changed, changeFrequency: "weekly", priority: .8 },
    ...docs.map((doc) => ({
      url: `${base}/docs/${doc.slug}/`,
      lastModified: changed,
      changeFrequency: "monthly" as const,
      priority: .7,
    })),
  ];
}
