"use client";

import { useEffect, useRef, useState } from "react";
import { Download, X } from "lucide-react";

type Kind = "installer" | "portable";
type Started = { name: string; url: string; kind: Kind };

/**
 * O que fazer depois do clique em baixar: a página dá lugar a três passos e a
 * um navegador desenhado apontando para o arquivo que acabou de chegar.
 *
 * O link continua sendo o arquivo (o download começa sem depender deste
 * componente); aqui só se escuta o clique nos links com `data-download`. O
 * `<dialog>` nativo faz o modal: fundo inerte, Esc fecha e o foco volta ao
 * botão, sem pagar uma biblioteca no orçamento de JS da página.
 */
export function AfterDownload({ signed }: { signed: boolean }) {
  const [started, setStarted] = useState<Started | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const d = dialog.current;
    if (started && d && !d.open) d.showModal();
  }, [started]);

  useEffect(() => {
    const onClick = (event: MouseEvent) => {
      // Ctrl/Shift/clique do meio abrem noutra aba: não é um download aqui.
      if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      const link = (event.target as Element | null)?.closest<HTMLAnchorElement>("a[data-download]");
      if (!link?.dataset.download) return;
      setStarted({
        name: link.dataset.download,
        url: link.href,
        kind: link.dataset.kind === "portable" ? "portable" : "installer",
      });
    };
    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, []);

  return (
    <dialog ref={dialog} className="after-download" aria-labelledby="after-title" aria-describedby="after-steps" onClose={(event) => {
        // O `close` de um fechamento anterior pode chegar depois de reabrir.
        if (!event.currentTarget.open) setStarted(null);
      }}>
        {started ? (
          <>
            <div className="after-copy">
              <span className="brand after-brand" aria-hidden="true">ISPer<span>.</span></span>
              <h2 id="after-title" className="after-title">
                Abra o ISPer <em>em 3 passos</em>
              </h2>
              <ol id="after-steps" className="after-steps">
                <li>
                  <h3>Abra os downloads</h3>
                  <p><kbd>Ctrl</kbd> + <kbd>J</kbd> no navegador, ou a seta de download no canto da janela.</p>
                </li>
                {started.kind === "installer" ? (
                  <>
                    <li>
                      <h3>Clique duas vezes no instalador</h3>
                      <p><code>{started.name}</code></p>
                      {signed ? null : (
                        <p className="after-note">
                          Se aparecer “O Windows protegeu o computador”, clique em <strong>Mais informações</strong> e em <strong>Executar assim mesmo</strong>. O instalador ainda não tem certificado; o aviso é esperado.
                        </p>
                      )}
                    </li>
                    <li>
                      <h3>Faça a primeira configuração</h3>
                      <p>Microfone, atalho e o modelo de transcrição, uma vez só. Depois, em qualquer app: segure <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Espaço</kbd>, fale e solte.</p>
                    </li>
                  </>
                ) : (
                  <>
                    <li>
                      <h3>Extraia a pasta</h3>
                      <p>Clique com o botão direito em <code>{started.name}</code> e escolha <strong>Extrair tudo</strong>.</p>
                    </li>
                    <li>
                      <h3>Abra o <code>isper-app.exe</code></h3>
                      <p>Dentro da pasta extraída. Não apague o <code>portable.txt</code>: é ele que avisa das versões novas.</p>
                    </li>
                  </>
                )}
              </ol>
              <p className="after-retry">
                Não começou? <a className="text-link" href={started.url}>Baixar de novo</a>
              </p>
              <button type="button" className="text-link after-back" onClick={() => dialog.current?.close()}>
                Voltar para a página de download
              </button>
            </div>

            <BrowserScene name={started.name} />

            <button type="button" className="after-close" aria-label="Fechar" onClick={() => dialog.current?.close()}>
              <X aria-hidden="true" />
            </button>
          </>
        ) : null}
    </dialog>
  );
}

/** Um navegador desenhado: o botão de downloads aceso, o arquivo na lista e o
 *  cursor indo até ele. Decorativo; os passos ao lado dizem tudo em texto. */
function BrowserScene({ name }: { name: string }) {
  return (
    <div className="after-visual" aria-hidden="true">
      <div className="after-browser">
        <div className="after-bar">
          <span className="after-dots"><i /><i /><i /></span>
          <span className="after-url">isper.pages.dev/download</span>
          <span className="after-dl"><Download /></span>
        </div>
        <div className="after-page">
          <i /><i /><i />
        </div>
        <div className="after-popover">
          <span className="after-popover-head">Downloads recentes</span>
          <div className="after-file">
            <span className="after-file-icon"><Download /></span>
            <span className="after-file-name">{name}</span>
          </div>
        </div>
        <svg className="after-cursor" viewBox="0 0 24 24">
          <path d="M5 3l14 8.2-6.1 1.3 3.5 6.6-2.6 1.4-3.5-6.7L5 18.2z" />
        </svg>
      </div>
    </div>
  );
}
