"use client";

import { Menu, X } from "lucide-react";
import { useEffect, useState } from "react";
import { usePathname } from "next/navigation";
import { GithubMark } from "@/components/icons/github-mark";
import { RouteLink } from "@/components/motion/route-link";
import { navItems, siteConfig } from "@/lib/site";

/** A same-page anchor is never "where you are"; a route is. */
function isCurrent(pathname: string, href: string) {
  if (href.includes("#")) return false;
  return pathname === href || pathname.startsWith(href.endsWith("/") ? href : `${href}/`);
}

export function SiteHeader() {
  const pathname = usePathname();
  const [open, setOpen] = useState(false);
  const [scrolled, setScrolled] = useState(false);

  useEffect(() => {
    const root = document.documentElement;
    const onScroll = () => {
      const limit = document.documentElement.scrollHeight - window.innerHeight;
      root.style.setProperty("--scroll-progress", String(limit > 0 ? Math.min(1, window.scrollY / limit) : 0));
      setScrolled(window.scrollY > 16);
    };
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll, { passive: true });
    return () => {
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
    };
  }, []);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => event.key === "Escape" && setOpen(false);
    document.body.dataset.menuOpen = "true";
    window.addEventListener("keydown", onKey);

    // Widening past the breakpoint takes the close control away with it, so the
    // menu closes itself rather than leaving the page locked behind two navs.
    const desktop = window.matchMedia("(min-width: 981px)");
    const onBreakpoint = (event: MediaQueryListEvent) => event.matches && setOpen(false);
    desktop.addEventListener("change", onBreakpoint);

    return () => {
      delete document.body.dataset.menuOpen;
      window.removeEventListener("keydown", onKey);
      desktop.removeEventListener("change", onBreakpoint);
    };
  }, [open]);

  return (
    <header className={`site-header ${scrolled ? "is-scrolled" : ""}`} style={{ viewTransitionName: "site-header" }}>
      <div className="shell header-inner">
        <RouteLink href="/" className="brand" aria-label="ISPer — início">ISPer<span>.</span></RouteLink>
        <nav className="desktop-nav" aria-label="Navegação principal">
          {navItems.map((item) => (
            <RouteLink key={item.href} href={item.href} aria-current={isCurrent(pathname, item.href) ? "page" : undefined}>
              {item.label}
            </RouteLink>
          ))}
        </nav>
        <div className="header-actions">
          <a className="github-link" href={siteConfig.repository} target="_blank" rel="noreferrer noopener">
            <GithubMark />
            <span>GitHub</span>
            <span className="visually-hidden">(abre em nova aba)</span>
          </a>
          <RouteLink className="button button-primary header-download" href="/download/">Baixar</RouteLink>
          <button className="menu-button" type="button" aria-label={open ? "Fechar menu" : "Abrir menu"} aria-expanded={open} aria-controls="mobile-navigation" onClick={() => setOpen((value) => !value)}>
            {open ? <X aria-hidden="true" /> : <Menu aria-hidden="true" />}
          </button>
        </div>
      </div>
      <div className="header-progress" aria-hidden="true"><i /></div>
      <nav id="mobile-navigation" className={`mobile-nav ${open ? "is-open" : ""}`} aria-label="Navegação móvel" hidden={!open}>
        {navItems.map((item) => (
          <RouteLink key={item.href} href={item.href} aria-current={isCurrent(pathname, item.href) ? "page" : undefined} onClick={() => setOpen(false)}>
            {item.label}
          </RouteLink>
        ))}
        <a href={siteConfig.repository} target="_blank" rel="noreferrer noopener"><GithubMark />Repositório no GitHub</a>
      </nav>
    </header>
  );
}
