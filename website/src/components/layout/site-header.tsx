"use client";

import Link from "next/link";
import { Menu, X } from "lucide-react";
import { useEffect, useState } from "react";
import { GithubMark } from "@/components/icons/github-mark";
import { navItems, siteConfig } from "@/lib/site";

export function SiteHeader() {
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
    return () => {
      delete document.body.dataset.menuOpen;
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <header className={`site-header ${scrolled ? "is-scrolled" : ""}`}>
      <div className="shell header-inner">
        <Link href="/" className="brand" aria-label="ISPer — início">ISPer<span>.</span></Link>
        <nav className="desktop-nav" aria-label="Navegação principal">
          {navItems.map((item) => <Link key={item.href} href={item.href}>{item.label}</Link>)}
        </nav>
        <div className="header-actions">
          <a className="github-link" href={siteConfig.repository} target="_blank" rel="noreferrer noopener">
            <GithubMark />
            <span>GitHub</span>
            <span className="visually-hidden">(abre em nova aba)</span>
          </a>
          <Link className="button button-primary header-download" href="/download/">Baixar</Link>
          <button className="menu-button" type="button" aria-label={open ? "Fechar menu" : "Abrir menu"} aria-expanded={open} aria-controls="mobile-navigation" onClick={() => setOpen((value) => !value)}>
            {open ? <X aria-hidden="true" /> : <Menu aria-hidden="true" />}
          </button>
        </div>
      </div>
      <div className="header-progress" aria-hidden="true"><i /></div>
      <nav id="mobile-navigation" className={`mobile-nav ${open ? "is-open" : ""}`} aria-label="Navegação móvel" hidden={!open}>
        {navItems.map((item) => <Link key={item.href} href={item.href} onClick={() => setOpen(false)}>{item.label}</Link>)}
        <a href={siteConfig.repository} target="_blank" rel="noreferrer noopener"><GithubMark />Repositório no GitHub</a>
      </nav>
    </header>
  );
}
