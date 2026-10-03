import type { Metadata } from "next";
import Link from "next/link";
import { ArrowRight, Check, Code2, Cpu, Download, FileCheck2, FolderArchive, LifeBuoy, MonitorCog, ShieldCheck, TriangleAlert } from "lucide-react";
import { IntegrityCheck } from "@/components/download/integrity-check";
import { RecommendedDownload } from "@/components/download/recommended-download";
import { variantProfile } from "@/lib/download-profile";
import {
  currentRelease,
  downloadVariants,
  formatBytes,
  formatReleaseDate,
  portableVariants,
  releaseIntegrityNotice,
  releaseLinks,
} from "@/lib/releases";
import { siteConfig } from "@/lib/site";
import { PageTransition } from "@/components/motion/page-transition";

export const metadata: Metadata = {
  title: "Download",
  description:
    "Baixe o ISPer para Windows 10 e 11: a página recomenda a versão certa para o seu PC (CPU ou GPU NVIDIA), mostra como instalar e o que fazer se algo der errado.",
  alternates: { canonical: "/download/" },
};

export default function DownloadPage() {
  // Todo instalador precisa estar assinado para a página parar de avisar.
  const signed = downloadVariants.length > 0 && downloadVariants.every((asset) => asset.authenticodeStatus === "verified");
  const published = currentRelease.publishedAt ? formatReleaseDate(currentRelease.publishedAt) : null;
  const portableOf = (variant: string) => portableVariants.find((asset) => asset.variant === variant);

  return (
    <PageTransition>
      <main id="conteudo" className="download-page shell">
        <header className="download-head">
          <div>
            <h1>Baixe o ISPer para Windows.</h1>
            <p className="section-lead">
              Gratuito e open source. Já escolhemos a versão para este computador: o motivo está embaixo do botão.
            </p>
          </div>
          <dl className="release-meta">
            <div><dt>Versão</dt><dd>{currentRelease.tag}</dd></div>
            {published ? <div><dt>Publicada</dt><dd>{published}</dd></div> : null}
            <div><dt>Sistema</dt><dd>Windows 10 e 11</dd></div>
            <a className="text-link" href={releaseLinks.current}>O que há de novo <ArrowRight aria-hidden="true" /></a>
          </dl>
        </header>

        <RecommendedDownload assets={downloadVariants} version={currentRelease.version} />

        {/* As duas versões lado a lado, com o que cada uma exige de verdade. Os
            requisitos são os do build: sem AVX2, ou com uma placa que a versão
            CUDA não cobre, o app fecha na primeira transcrição. */}
        <section className="download-section" aria-labelledby="escolher">
          <h2 id="escolher" className="download-h2">Qual versão escolher</h2>
          <div className="compare-grid">
            {downloadVariants.map((asset) => {
              const profile = variantProfile[asset.variant];
              const zip = portableOf(asset.variant);
              return (
                <article key={asset.variant} className="compare-card">
                  <header className="compare-head">
                    {asset.variant === "cuda" ? <MonitorCog aria-hidden="true" /> : <Cpu aria-hidden="true" />}
                    <h3>{profile.title}</h3>
                  </header>
                  <p className="compare-audience">{profile.audience}</p>
                  <ul className="compare-reqs" aria-label="Requisitos">
                    {profile.requirements.map((item) => (
                      <li key={item.text} className={item.caution ? "is-caution" : undefined}>
                        {item.caution ? <TriangleAlert aria-hidden="true" /> : <Check aria-hidden="true" />}{item.text}
                      </li>
                    ))}
                  </ul>
                  <p className="compare-model">{profile.model}</p>
                  <div className="compare-actions">
                    <a className="button button-secondary" href={asset.downloadUrl}>
                      <Download aria-hidden="true" />Instalador<span className="compare-size">{formatBytes(asset.sizeBytes)}</span>
                    </a>
                    {zip ? (
                      <a className="text-link" href={zip.downloadUrl}>
                        <FolderArchive aria-hidden="true" />Portátil (zip) · {formatBytes(zip.sizeBytes)}
                      </a>
                    ) : null}
                  </div>
                </article>
              );
            })}
          </div>
          <p className="download-footnote">
            A versão portátil é a pasta do ISPer inteira, sem instalador e sem administrador: descompacte e abra o <code>isper-app.exe</code>. Não apague o <code>portable.txt</code>; é ele que faz o ISPer avisar das versões novas sem instalar por cima.
          </p>
        </section>

        <section className="download-section" aria-labelledby="instalar">
          <h2 id="instalar" className="download-h2">Como instalar</h2>
          <ol className="install-steps">
            <li>
              <h3>Abra o instalador</h3>
              {signed ? (
                <p>O instalador é assinado, então o Windows o abre sem avisos.</p>
              ) : (
                <p>
                  O instalador ainda não tem certificado, então o Windows mostra “O Windows protegeu o computador”. Clique em <strong>Mais informações</strong> e depois em <strong>Executar assim mesmo</strong>. É esperado.
                </p>
              )}
            </li>
            <li>
              <h3>Instale, sem administrador</h3>
              <p>O ISPer vai para a sua pasta de usuário e não pede permissão de administrador. Ao terminar, ele fica na bandeja, perto do relógio.</p>
            </li>
            <li>
              <h3>Faça a primeira configuração</h3>
              <p>Escolha o microfone e o atalho, e baixe o modelo de transcrição, uma vez só. Depois, em qualquer aplicativo: segure <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Espaço</kbd>, fale e solte.</p>
            </li>
          </ol>
          <Link className="text-link" href="/docs/primeiros-passos/instalacao/">Guia de instalação completo <ArrowRight aria-hidden="true" /></Link>
        </section>

        <section className="download-section" aria-labelledby="problemas">
          <h2 id="problemas" className="download-h2"><LifeBuoy aria-hidden="true" />Se algo der errado</h2>
          <div className="faq-list">
            <details>
              <summary>O ISPer fecha quando começo a gravar ou a ditar</summary>
              <p>
                Instale a versão mais recente, por esta página. Versões antigas tinham defeitos já corrigidos: a 0.17.0, por exemplo, fechava na primeira transcrição em PCs sem AVX-512, que são a maioria.
              </p>
              <p>
                Se continuar, confira os requisitos da sua versão. A versão CPU precisa de um processador com AVX2. A versão CUDA desta release só roda em placas NVIDIA RTX 40 ou mais novas: com uma RTX 20, RTX 30 ou outra placa, instale a versão CPU.
              </p>
            </details>
            <details>
              <summary>O Windows não deixa abrir o instalador</summary>
              <p>
                É o SmartScreen, porque o instalador ainda não tem certificado. Na janela “O Windows protegeu o computador”, clique em <strong>Mais informações</strong> e em <strong>Executar assim mesmo</strong>. Para ter certeza de que o arquivo é o publicado, confira a soma SHA-256 mais abaixo.
              </p>
            </details>
            <details>
              <summary>A versão CUDA não está usando a placa de vídeo</summary>
              <p>
                Atualize o driver da NVIDIA e reabra o ISPer. Em Configurações → Sistema → Diagnóstico, ele mostra o motor em uso e as DLLs do CUDA.{" "}
                <Link className="text-link" href="/docs/solucao-de-problemas/cuda/">Diagnosticar a aceleração NVIDIA</Link>
              </p>
            </details>
            <details>
              <summary>Preciso de ajuda</summary>
              <p>
                Em Configurações → Sistema, <strong>Exportar diagnóstico</strong> gera um .zip com as versões, a configuração e os últimos logs, sem nada do que você ditou. Anexe-o a uma{" "}
                <a className="text-link" href={siteConfig.issues}>issue no GitHub</a>.
              </p>
            </details>
          </div>
        </section>

        <div className="download-grid">
          <section className="panel" aria-labelledby="conferir">
            <h2 id="conferir"><FileCheck2 aria-hidden="true" />Conferir o download</h2>
            <p className="panel-note">Opcional: prova que o arquivo é exatamente o publicado na release.</p>
            <IntegrityCheck assets={[...downloadVariants, ...portableVariants]} />
          </section>

          <div className="download-stack">
            <section className="panel" aria-labelledby="novidades">
              <h2 id="novidades"><ShieldCheck aria-hidden="true" />Novidades da {currentRelease.tag}</h2>
              <ul className="marked-list">
                {currentRelease.notes.slice(0, 4).map((note) => <li key={note}>{note.replace(/^\p{L}+ — /u, "")}</li>)}
              </ul>
              <p className="panel-note">{releaseIntegrityNotice}</p>
              <div className="panel-links">
                <a className="text-link" href={releaseLinks.current}>Todas as notas <ArrowRight aria-hidden="true" /></a>
                <a className="text-link" href={releaseLinks.all}>Versões anteriores <ArrowRight aria-hidden="true" /></a>
              </div>
            </section>

            <section className="panel" aria-labelledby="codigo">
              <h2 id="codigo"><Code2 aria-hidden="true" />Compilar a partir do código</h2>
              <p className="panel-note">Rust, Visual Studio Build Tools e CMake, e um comando.</p>
              <a className="text-link" href={`${siteConfig.repository}/blob/main/docs/DESENVOLVIMENTO.md`}>Guia de desenvolvimento <ArrowRight aria-hidden="true" /></a>
            </section>
          </div>
        </div>
      </main>
    </PageTransition>
  );
}
