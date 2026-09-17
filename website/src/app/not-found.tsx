import Link from "next/link";
import { PageTransition } from "@/components/motion/page-transition";

export default function NotFound() {
  return (
    <PageTransition>
    <main id="conteudo" className="mx-auto flex min-h-[70svh] max-w-3xl flex-col justify-center px-4 py-16 text-center">
      <p className="font-mono text-sm text-[var(--accent-2)]">404</p>
      <h1 className="mt-3 font-display text-5xl font-semibold">Página não encontrada.</h1>
      <p className="mt-4 text-[var(--ink-2)]">
        O conteúdo pode ter mudado de rota ou ainda não estar publicado nesta versão do portal.
      </p>
      <Link
        href="/docs/"
        className="mx-auto mt-8 rounded-xl bg-[var(--accent)] px-5 py-3 font-semibold text-[var(--accent-ink)]"
      >
        Abrir documentação
      </Link>
    </main>
    </PageTransition>
  );
}
