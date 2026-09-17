import Link from "next/link";
import { GithubMark } from "@/components/icons/github-mark";
import { siteConfig } from "@/lib/site";

export function SiteFooter() {
  return (
    <footer className="site-footer">
      <div className="shell footer-grid">
        <div><Link href="/" className="brand" aria-label="ISPer — início">ISPer<span>.</span></Link><p>Transcrição local para registrar ideias e reuniões sem transformar áudio confidencial em dado de terceiros.</p></div>
        <div><h2>Produto</h2><Link href="/#recursos">Recursos</Link><Link href="/download/">Download</Link><a href={siteConfig.releases}>Releases</a><a href={`${siteConfig.repository}/blob/main/CHANGELOG.md`}>Changelog</a></div>
        <div><h2>Aprender</h2><Link href="/docs/">Documentação</Link><Link href="/docs/primeiros-passos/instalacao/">Instalação</Link><Link href="/docs/reunioes-e-sistema/privacidade/">Privacidade</Link><Link href="/docs/solucao-de-problemas/faq/">FAQ</Link></div>
        <div><h2>Comunidade</h2><a href={siteConfig.repository} className="footer-github"><GithubMark />GitHub</a><a href={siteConfig.issues}>Issues</a><a href={`${siteConfig.repository}/blob/main/CONTRIBUTING.md`}>Contribuir</a><a href={`${siteConfig.repository}/blob/main/SECURITY.md`}>Segurança</a></div>
      </div>
      <div className="shell footer-bottom"><span>© 2026 ISPer · código aberto sob licença MIT</span><span>Feito no Brasil para Windows</span></div>
    </footer>
  );
}
