/**
 * The one picture the portal was missing.
 *
 * The product's irreducible claim is that audio never leaves the machine, and
 * until now that sentence lived only in prose inside a collapsed FAQ item at
 * 78% scroll depth. This draws the boundary: everything that touches audio sits
 * inside it, and the only thing that ever crosses is text the reader has
 * explicitly turned on.
 *
 * Two authored variants rather than one scaled diagram — a horizontal flow
 * illegible at 380px is not a diagram, it is a texture. Both are aria-hidden;
 * the figure carries one text equivalent for both.
 */

/**
 * SVG text does not wrap, so every string here is sized to its box: the title
 * fits 22 characters at 0.95rem and the detail 30 at 0.8rem inside a 240px box.
 */
const stages = [
  { title: "Microfone e sistema", detail: "captura local, sem bot" },
  { title: "whisper.cpp", detail: "transcrição e diarização" },
  { title: "SQLite", detail: "histórico e busca no disco" },
];

const cloud = { title: "Provedor de IA que você configurar", detail: "Groq · Gemini · Claude" };

export function AudioBoundary() {
  return (
    <figure className="boundary">
      <svg className="boundary-wide" viewBox="0 0 1100 272" aria-hidden="true" focusable="false">
        <defs>
          <marker id="bd-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M0 0 10 5 0 10z" fill="var(--accent)" />
          </marker>
          <marker id="bd-arrow-out" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M0 0 10 5 0 10z" fill="var(--muted)" />
          </marker>
        </defs>

        <rect x="8" y="8" width="812" height="256" rx="16" fill="rgba(240,126,114,.04)" stroke="var(--line-2)" />
        <text className="bd-zone" x="30" y="40">Seu computador · Windows</text>

        {stages.map((stage, index) => {
          const x = 30 + index * 264;
          return (
            <g key={stage.title}>
              <rect x={x} y="76" width="240" height="96" rx="12" fill="var(--panel-2)" stroke="var(--line)" />
              <text className="bd-title" x={x + 20} y="112">{stage.title}</text>
              <text className="bd-detail" x={x + 20} y="140">{stage.detail}</text>
              {index < stages.length - 1 ? (
                <line x1={x + 244} y1="124" x2={x + 260} y2="124" stroke="var(--accent)" strokeWidth="2" markerEnd="url(#bd-arrow)" />
              ) : null}
            </g>
          );
        })}

        <text className="bd-flow" x="30" y="216">áudio · nunca atravessa esta linha</text>
        <line x1="820" y1="8" x2="820" y2="264" stroke="var(--accent)" strokeWidth="2" strokeDasharray="2 6" />

        <line x1="820" y1="124" x2="852" y2="124" stroke="var(--muted)" strokeWidth="2" strokeDasharray="6 5" markerEnd="url(#bd-arrow-out)" />
        <rect x="860" y="76" width="232" height="96" rx="12" fill="var(--bg-2)" stroke="var(--line)" />
        <text className="bd-title" x="880" y="106">Provedor de IA</text>
        <text className="bd-title" x="880" y="128">que você configurar</text>
        <text className="bd-detail" x="880" y="154">{cloud.detail}</text>
        <text className="bd-flow bd-flow-out" x="860" y="216">só o texto, se você ativar</text>
      </svg>

      <svg className="boundary-narrow" viewBox="0 0 340 520" aria-hidden="true" focusable="false">
        <defs>
          <marker id="bdn-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M0 0 10 5 0 10z" fill="var(--accent)" />
          </marker>
          <marker id="bdn-arrow-out" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M0 0 10 5 0 10z" fill="var(--muted)" />
          </marker>
        </defs>

        <rect x="6" y="6" width="328" height="368" rx="14" fill="rgba(240,126,114,.04)" stroke="var(--line-2)" />
        <text className="bd-zone" x="22" y="32">Seu computador · Windows</text>

        {stages.map((stage, index) => {
          const y = 48 + index * 104;
          return (
            <g key={stage.title}>
              <rect x="22" y={y} width="296" height="78" rx="12" fill="var(--panel-2)" stroke="var(--line)" />
              <text className="bd-title" x="40" y={y + 32}>{stage.title}</text>
              <text className="bd-detail" x="40" y={y + 56}>{stage.detail}</text>
              {index < stages.length - 1 ? (
                <line x1="170" y1={y + 82} x2="170" y2={y + 100} stroke="var(--accent)" strokeWidth="2" markerEnd="url(#bdn-arrow)" />
              ) : null}
            </g>
          );
        })}

        <line x1="6" y1="374" x2="334" y2="374" stroke="var(--accent)" strokeWidth="2" strokeDasharray="2 6" />
        <text className="bd-flow" x="170" y="396" textAnchor="middle">áudio nunca atravessa esta linha</text>

        <line x1="170" y1="404" x2="170" y2="424" stroke="var(--muted)" strokeWidth="2" strokeDasharray="6 5" markerEnd="url(#bdn-arrow-out)" />
        <rect x="22" y="430" width="296" height="80" rx="12" fill="var(--bg-2)" stroke="var(--line)" />
        <text className="bd-title" x="40" y="458">Provedor de IA que você configurar</text>
        <text className="bd-detail" x="40" y="480">{cloud.detail}</text>
        <text className="bd-flow bd-flow-out" x="40" y="500">só o texto, se você ativar</text>
      </svg>

      <figcaption>
        O áudio é capturado, transcrito e guardado sem sair do seu computador. Se você configurar um provedor de IA para resumos, só o texto necessário atravessa a linha — e apenas depois que você ativa.
      </figcaption>
    </figure>
  );
}
