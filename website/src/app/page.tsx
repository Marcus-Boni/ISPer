import type { Metadata } from "next";
import dynamic from "next/dynamic";
import Link from "next/link";
import { ArrowRight, AudioLines, BookOpen, BrainCircuit, Check, Code2, Cpu, Download, Keyboard, LockKeyhole, Mic2, Search, ShieldCheck, Sparkles, Users } from "lucide-react";
import { CopyCommand } from "@/components/landing/copy-command";
import { InteractiveStage } from "@/components/landing/interactive-stage";
import { RevealEffects } from "@/components/landing/reveal-effects";
import { PageTransition } from "@/components/motion/page-transition";
import { siteConfig } from "@/lib/site";

const BenchmarkPanel = dynamic(() => import("@/components/landing/benchmark-panel").then((module) => module.BenchmarkPanel), {
  loading: () => <div className="benchmark-panel chart-placeholder" aria-label="Carregando a comparação de desempenho" />,
});

export const metadata: Metadata = { alternates: { canonical: "/" } };

type Verdict = "good" | "bad" | "neutral";

const verdictLabel: Record<Verdict, string> = { good: "A favor", bad: "Contra", neutral: "Depende" };

/** The verdict is stated per cell because the same word flips meaning by row. */
const comparison: Array<{ criterion: string; cells: Array<{ text: string; verdict: Verdict }> }> = [
  { criterion: "Áudio enviado para transcrever", cells: [
    { text: "Não", verdict: "good" }, { text: "Sim", verdict: "bad" }, { text: "Sim", verdict: "bad" }] },
  { criterion: "Mensalidade obrigatória", cells: [
    { text: "Não", verdict: "good" }, { text: "Por uso", verdict: "bad" }, { text: "Geralmente", verdict: "bad" }] },
  { criterion: "Bot entra na reunião", cells: [
    { text: "Não", verdict: "good" }, { text: "Não se aplica", verdict: "neutral" }, { text: "Frequentemente", verdict: "bad" }] },
  { criterion: "Funciona sem internet após configurar", cells: [
    { text: "Transcrição: sim", verdict: "good" }, { text: "Não", verdict: "bad" }, { text: "Não", verdict: "bad" }] },
  { criterion: "Resumo", cells: [
    { text: "Provider opcional", verdict: "neutral" }, { text: "Conforme API", verdict: "neutral" }, { text: "Incluso no serviço", verdict: "neutral" }] },
];

const faq = [
  ["O áudio sai do meu computador?", "A transcrição e a diarização rodam no Windows. Se você ativar um provedor de IA para resumos, o texto necessário é enviado ao provedor escolhido; o áudio não é enviado para transcrição."],
  ["Preciso de uma GPU NVIDIA?", "Não. A edição CPU funciona sem GPU dedicada. A edição CUDA acelera modelos maiores em hardware NVIDIA compatível."],
  ["Funciona com o Microsoft Teams?", "Sim. O ISPer usa o loopback de processo do Windows para capturar o áudio da chamada e combina esse canal com o seu microfone."],
  ["O ISPer tem mensalidade?", "Não. O software é open source sob licença MIT. Serviços opcionais de IA podem ter custos próprios conforme o provedor configurado."],
];

export default function Home() {
  const softwareSchema = {
    "@context": "https://schema.org", "@type": "SoftwareApplication", name: "ISPer", applicationCategory: "BusinessApplication", operatingSystem: "Windows 10, Windows 11", softwareVersion: siteConfig.currentVersion.replace("v", ""), description: siteConfig.description, url: siteConfig.url, downloadUrl: siteConfig.latestRelease, license: `${siteConfig.repository}/blob/main/LICENSE`, offers: { "@type": "Offer", price: "0", priceCurrency: "BRL" },
  };
  const faqSchema = { "@context": "https://schema.org", "@type": "FAQPage", mainEntity: faq.map(([question, answer]) => ({ "@type": "Question", name: question, acceptedAnswer: { "@type": "Answer", text: answer } })) };

  return (
    <PageTransition>
    <main id="conteudo">
      <RevealEffects />
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(softwareSchema).replace(/</g, "\\u003c") }} />
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(faqSchema).replace(/</g, "\\u003c") }} />

      <section className="hero shell">
        <div className="hero-copy">
          <h1>
            <span className="hero-line-mask"><span className="hero-line">Suas palavras.</span></span>
            <span className="hero-line-mask"><span className="hero-line"><em>No seu computador.</em></span></span>
          </h1>
          <p className="hero-lead">Dite em qualquer aplicativo e transcreva reuniões com IA local. Sem mensalidade e sem enviar seu áudio para uma API.</p>
          <div className="hero-actions">
            <Link className="button button-primary button-large" href="/download/" transitionTypes={["nav-forward"]}><Download aria-hidden="true" />Escolher instalador<span className="button-tag">{siteConfig.currentVersion}</span></Link>
            <Link className="button button-secondary button-large" href="/docs/primeiros-passos/instalacao/" transitionTypes={["nav-forward"]}><BookOpen aria-hidden="true" />Ver instalação</Link>
          </div>
          <ul className="hero-proof">
            <li><ShieldCheck aria-hidden="true" />Open source MIT</li>
            <li><Cpu aria-hidden="true" />Instalador x64 · CPU ou CUDA para NVIDIA</li>
            <li><Users aria-hidden="true" />Sem bot na chamada</li>
          </ul>
          <CopyCommand />
        </div>
        <div className="hero-stage"><InteractiveStage /></div>
      </section>

      <section className="trust-strip" aria-label="Características essenciais">
        <div className="shell trust-grid"><span><LockKeyhole aria-hidden="true" />Transcrição local</span><span><Mic2 aria-hidden="true" />Ditado em qualquer app</span><span><Users aria-hidden="true" />Falantes separados</span><span><Search aria-hidden="true" />Histórico pesquisável</span></div>
      </section>

      <section id="recursos" className="section shell" data-reveal>
        <div className="section-heading"><h2>Um fluxo contínuo entre falar, registrar e encontrar.</h2><p>O ISPer vive na bandeja do Windows. Você chama quando precisa e volta ao trabalho sem trocar de contexto.</p></div>
        <div className="bento-grid">
          <article className="feature feature-wide feature-dictation" data-reveal-item><div className="feature-copy"><span className="feature-icon"><Keyboard aria-hidden="true" /></span><h3>Fale onde o cursor estiver</h3><p>Segure o atalho global, fale e solte. O texto é colado no campo em foco e seu clipboard anterior é restaurado.</p><div className="app-row"><span>Teams</span><span>Word</span><span>Terminal</span><span>Notion</span><span>WhatsApp</span></div></div><div className="typed-note"><span className="caret" />A próxima versão entra em homologação na sexta-feira.</div></article>
          <article className="feature feature-meeting" data-reveal-item><span className="feature-icon"><AudioLines aria-hidden="true" /></span><h3>Reuniões sem um bot na sala</h3><p>O loopback captura a chamada; o microfone identifica você. A diarização local organiza o restante por participante.</p><div className="speaker-stack"><span className="p1">Participante 1</span><span className="p2">Participante 2</span><span className="me">Eu</span></div></article>
          <article className="feature feature-intelligence" data-reveal-item><span className="feature-icon"><BrainCircuit aria-hidden="true" /></span><h3>IA sob sua escolha</h3><p>Resumos opcionais com Groq, Gemini ou Claude. Se a API falhar, o texto original continua preservado.</p><ul className="feature-list"><li><Check aria-hidden="true" />Transcrição e diarização locais</li><li><Check aria-hidden="true" />Chaves no Credential Manager</li><li><Check aria-hidden="true" />Revisão antes de usar</li></ul></article>
          <article className="feature feature-wide feature-search" data-reveal-item><div className="feature-copy"><span className="feature-icon"><Search aria-hidden="true" /></span><h3>Encontre pelo sentido, não só pela palavra.</h3><p>A busca semântica percorre reuniões e ditados no SQLite. Use Gemini ou um endpoint compatível com OpenAI, incluindo Ollama local.</p></div><div className="search-demo"><div className="search-demo-field"><Search aria-hidden="true" /><span>quando decidimos o prazo?</span><kbd>Enter</kbd></div><p><strong>Reunião de lançamento</strong><mark>“A próxima versão entra em homologação na sexta-feira.”</mark></p></div></article>
        </div>
      </section>

      <section className="stage-section" data-reveal>
        <div className="shell stage-section-grid"><div><h2>Uma biblioteca que se torna memória de trabalho.</h2><p className="section-lead">Reuniões, ditados, resumos e momentos marcados ficam organizados localmente. Pesquise, revise os falantes e volte ao ponto exato da conversa.</p><Link className="text-link" transitionTypes={["nav-forward"]} href="/docs/busca-semantica/configuracao/">Entender a busca semântica <ArrowRight aria-hidden="true" /></Link></div><div className="library-card" data-reveal-item><div className="library-head"><span>Biblioteca</span><div><Search aria-hidden="true" />buscar no título, resumo e transcript…</div></div><div className="library-list"><article><time>Hoje · 14:32</time><strong>Planejamento do lançamento</strong><p>3 participantes · 38 min · resumo pronto</p></article><article><time>Ontem · 09:10</time><strong>Revisão da documentação</strong><p>2 participantes · 24 min · 2 momentos</p></article><article><time>11 set · 16:45</time><strong>Notas por ditado</strong><p>12 trechos · processados localmente</p></article></div></div></div>
      </section>

      <section id="benchmarks" className="section shell" data-reveal>
        <div className="section-heading benchmark-heading"><div><h2>Desempenho que você consegue auditar.</h2><p className="section-lead">Mostramos o que foi medido, o que é cálculo e o que ainda precisa de benchmark. Sem transformar estimativa em promessa.</p></div><Link className="text-link" transitionTypes={["nav-forward"]} href="/docs/referencia/benchmarks/">Ver metodologia <ArrowRight aria-hidden="true" /></Link></div>
        <div data-reveal-item><BenchmarkPanel /></div>
        <div className="comparison-wrap" data-reveal-item><table className="comparison-table"><caption>Comparação de arquitetura e privacidade</caption><thead><tr><th scope="col">Critério</th><th scope="col" className="isper-col">ISPer local</th><th scope="col">API de transcrição</th><th scope="col">Notetaker corporativo</th></tr></thead><tbody>{comparison.map((row) => <tr key={row.criterion}><th scope="row">{row.criterion}</th>{row.cells.map((cell, index) => <td key={`${row.criterion}-${index}`} className={index === 0 ? "isper-col" : undefined}><span className={`verdict verdict-${cell.verdict}`}><span className="visually-hidden">{verdictLabel[cell.verdict]}: </span>{cell.text}</span></td>)}</tr>)}</tbody></table></div>
      </section>

      <section className="shortcut-section" data-reveal>
        <div className="shell shortcut-grid"><div><h2>O atalho desaparece. A ideia fica.</h2><p className="section-lead">O padrão é <strong>Ctrl + Alt + Espaço</strong>. Se houver conflito, o ISPer tenta combinações alternativas — e você pode gravar a sua nas Configurações.</p><Link className="text-link" transitionTypes={["nav-forward"]} href="/docs/solucao-de-problemas/atalhos/">Configurar atalhos <ArrowRight aria-hidden="true" /></Link></div><div className="key-combo" aria-label="Ctrl mais Alt mais Espaço"><kbd>Ctrl</kbd><span>+</span><kbd>Alt</kbd><span>+</span><kbd>Espaço</kbd></div></div>
      </section>

      <section className="section shell" data-reveal>
        <div className="section-heading"><h2>Perguntas antes do primeiro ditado.</h2><p>As respostas curtas estão aqui. Os detalhes operacionais ficam na documentação versionada.</p></div>
        <div className="faq-list">{faq.map(([question, answer]) => <details key={question} data-reveal-item><summary>{question}<i aria-hidden="true" /></summary><p>{answer}</p></details>)}</div>
      </section>

      <section className="final-cta shell" data-reveal><div><Sparkles aria-hidden="true" /><h2>Transforme fala em trabalho pronto.</h2><p className="section-lead">Baixe o ISPer, escolha um modelo e faça seu primeiro ditado em poucos minutos.</p></div><div className="final-actions"><Link className="button button-primary button-large" href="/download/" transitionTypes={["nav-forward"]}><Download aria-hidden="true" />Escolher instalador</Link><a className="button button-secondary button-large" href={siteConfig.repository}><Code2 aria-hidden="true" />Ver código</a></div></section>
    </main>
    </PageTransition>
  );
}
