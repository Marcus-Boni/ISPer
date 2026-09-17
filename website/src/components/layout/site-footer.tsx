import { RouteLink } from "@/components/motion/route-link";
import { GithubMark } from "@/components/icons/github-mark";
import { siteConfig } from "@/lib/site";

export function SiteFooter() {
  return (
    <footer className="site-footer">
      <div className="shell footer-grid">
        <div><RouteLink href="/" className="brand" aria-label="ISPer — início">ISPer<span>.</span></RouteLink><p>Transcrição local para registrar ideias e reuniões sem transformar áudio confidencial em dado de terceiros.</p></div>
        <div><h2>Produto</h2><RouteLink href="/#recursos">Recursos</RouteLink><RouteLink href="/download/">Download</RouteLink><a href={siteConfig.releases}>Releases</a><a href={`${siteConfig.repository}/blob/main/CHANGELOG.md`}>Changelog</a></div>
        <div><h2>Aprender</h2><RouteLink href="/docs/">Documentação</RouteLink><RouteLink href="/docs/primeiros-passos/instalacao/">Instalação</RouteLink><RouteLink href="/docs/reunioes-e-sistema/privacidade/">Privacidade</RouteLink><RouteLink href="/docs/solucao-de-problemas/faq/">FAQ</RouteLink></div>
        <div><h2>Comunidade</h2><a href={siteConfig.repository} className="footer-github"><GithubMark />GitHub</a><a href={siteConfig.issues}>Issues</a><a href={`${siteConfig.repository}/blob/main/CONTRIBUTING.md`}>Contribuir</a><a href={`${siteConfig.repository}/blob/main/SECURITY.md`}>Segurança</a></div>
      </div>
      <div className="shell footer-bottom"><span>© 2026 ISPer · código aberto sob licença MIT</span><span>Feito no Brasil para Windows</span></div>
    </footer>
  );
}
