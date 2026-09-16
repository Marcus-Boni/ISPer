const transcript = [
  {
    speaker: "Participante 1",
    color: "var(--p1)",
    text: "A meta e fechar a proposta ate sexta e deixar o onboarding pronto.",
  },
  {
    speaker: "Eu",
    color: "var(--p3)",
    text: "Combinado. Eu fico com o resumo executivo e marco o follow-up.",
  },
  {
    speaker: "Participante 2",
    color: "var(--p2)",
    text: "Vou enviar os arquivos de referencia e validar os nomes tecnicos.",
  },
];

export function AppStage() {
  return (
    <section className="relative overflow-hidden py-16 sm:py-20" id="demo">
      <div className="mx-auto grid max-w-7xl items-center gap-10 px-4 sm:px-6 lg:grid-cols-[0.92fr_1.08fr] lg:px-8">
        <div>
          <h2 className="font-display text-4xl font-semibold leading-tight text-[var(--ink)] sm:text-5xl">
            A reunião aparece como memória organizada, não como arquivo perdido.
          </h2>
          <p className="mt-5 max-w-xl text-lg leading-8 text-[var(--ink-2)]">
            A demonstracao usa dados sinteticos para mostrar a experiencia real:
            audio local, falas separadas por cor, resumo acionavel e busca no
            historico.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-3">
            {["Ctrl", "Shift", "Espaco"].map((key) => (
              <kbd
                key={key}
                className="rounded-lg border border-[var(--line-2)] bg-[var(--panel-2)] px-4 py-3 font-mono text-sm text-[var(--ink)] shadow-[inset_0_-3px_0_rgba(0,0,0,0.24)]"
              >
                {key}
              </kbd>
            ))}
            <span className="text-sm text-[var(--muted)]">
              atalho demonstrativo; o padrao do app pode ser configurado.
            </span>
          </div>
        </div>

        <div className="app-window overflow-hidden rounded-2xl border border-[var(--line)] bg-[var(--panel)] shadow-[0_26px_80px_-36px_rgba(0,0,0,0.9)]">
          <div className="flex items-center justify-between border-b border-[var(--line)] bg-[var(--panel-2)] px-4 py-3">
            <div className="flex gap-2" aria-hidden="true">
              <span className="h-3 w-3 rounded-full bg-[#f06f66]" />
              <span className="h-3 w-3 rounded-full bg-[#e8c15a]" />
              <span className="h-3 w-3 rounded-full bg-[#84c297]" />
            </div>
            <span className="font-mono text-xs text-[var(--muted)]">Biblioteca ISPer</span>
          </div>
          <div className="grid gap-0 md:grid-cols-[220px_1fr]">
            <aside className="border-b border-[var(--line)] bg-[var(--bg-2)] p-4 md:border-b-0 md:border-r">
              <div className="rounded-lg bg-[var(--accent-soft)] p-3 text-sm font-semibold text-[var(--accent-2)]">
                Reuniao semanal
              </div>
              <div className="mt-4 space-y-3 text-sm text-[var(--muted)]">
                <p>Hoje - 32 min</p>
                <p>3 participantes</p>
                <p>Resumo pronto</p>
              </div>
            </aside>
            <div className="p-5 sm:p-6">
              <div className="mb-5 flex flex-wrap gap-2">
                {["Transcript", "Resumo", "Decisoes", "Pendencias"].map((tab, index) => (
                  <span
                    key={tab}
                    className={`rounded-full border px-3 py-1 text-xs ${
                      index === 0
                        ? "border-[var(--accent)] bg-[var(--accent-soft)] text-[var(--accent-2)]"
                        : "border-[var(--line)] text-[var(--muted)]"
                    }`}
                  >
                    {tab}
                  </span>
                ))}
              </div>
              <div className="space-y-4">
                {transcript.map((line) => (
                  <div key={line.speaker} className="rounded-xl border border-[var(--line)] bg-[var(--panel-2)] p-4">
                    <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
                      <span className="h-2.5 w-2.5 rounded-full" style={{ background: line.color }} />
                      {line.speaker}
                    </div>
                    <p className="leading-7 text-[var(--ink-2)]">{line.text}</p>
                  </div>
                ))}
              </div>
              <div className="mt-5 rounded-xl border border-[var(--line)] bg-[rgba(124,196,240,0.08)] p-4">
                <p className="text-sm font-semibold text-[var(--info)]">Resumo automatico</p>
                <p className="mt-2 text-sm leading-6 text-[var(--ink-2)]">
                  Proposta ate sexta, arquivos de referencia pendentes e follow-up
                  marcado como proxima acao.
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
