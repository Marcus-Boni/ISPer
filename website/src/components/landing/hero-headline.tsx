import type { CSSProperties } from "react";
import { BRAND_MARK } from "@/lib/brand-mark";

/**
 * O título é ditado.
 *
 * O gesto que define o ISPer — segurar o atalho, falar, soltar, e o texto
 * aparecer onde você está escrevendo — era descrito três vezes na página e
 * mostrado nenhuma. Aqui ele acontece no próprio título, que é o único lugar
 * que todo visitante vê, inclusive no celular, onde o palco fica abaixo da dobra.
 *
 * Cada palavra passa pelos três estados que o produto de fato tem: não ouvida,
 * provisória (a legenda ao vivo) e final (o passe final, que varre o título
 * inteiro quando o atalho é solto). A marca, em miniatura, vai na frente como
 * cursor: barras falando, ponto de gravação aceso.
 *
 * Tudo em CSS, sem JavaScript. A sequência começa no primeiro quadro em que a
 * folha de estilos chega e não espera hidratação nem `requestAnimationFrame` —
 * os mesmos que, suspensos, já deixaram o hero preso no estado inicial. E nenhum
 * estado esconde o texto: "não ouvida" é translúcida, não invisível, então o
 * título é pintado já no primeiro quadro e o LCP fecha ali, não quando a última
 * palavra pousa. O leitor de tela lê o h1 inteiro desde o começo; a marca é
 * `aria-hidden`. Com movimento reduzido, o título simplesmente está lá.
 */

/** Quando cada palavra é dita, em segundos — o ritmo de uma frase falada em pt-BR. */
const LINES = [
  { emphasis: false, words: [{ text: "Suas", at: 0.32 }, { text: "palavras.", at: 0.58 }] },
  // A pausa entre as frases é o fôlego: o ponto segue aceso, a onda baixa.
  { emphasis: true, words: [{ text: "No", at: 1.3 }, { text: "seu", at: 1.47 }, { text: "computador.", at: 1.66 }] },
];

/** O atalho é solto aqui: o passe final começa e o cursor se recolhe. */
const RELEASE = 2.4;
/** O passe final varre da esquerda para a direita, palavra a palavra. */
const SWEEP = 0.05;
/** A marca fica na última palavra até a voz assentar, e então cede ao cursor de texto. */
const SETTLE = 0.22;

type Timed = { text: string; at: number; next: number; index: number; last: boolean };

function timeline(): Timed[][] {
  const flat = LINES.flatMap((line) => line.words);
  let index = 0;
  return LINES.map((line) =>
    line.words.map((word) => {
      const i = index++;
      const next = flat[i + 1]?.at ?? RELEASE;
      return { ...word, next, index: i, last: i === flat.length - 1 };
    }),
  );
}

const seconds = (value: number) => `${value.toFixed(3)}s`;

/** A marca do ISPer como cursor: a mesma geometria do ícone, lida do módulo gerado. */
function Cursor({ last }: { last: boolean }) {
  return (
    <svg className={`word-cursor${last ? " is-last" : ""}`} viewBox={BRAND_MARK.viewBox} aria-hidden="true" focusable="false">
      <g className="word-cursor-voice">
        {BRAND_MARK.bars.map((bar, i) => (
          <rect key={bar.x} className={`bar-${i}`} x={bar.x} y={bar.y} width={bar.width} height={bar.height} rx={bar.width / 2} />
        ))}
      </g>
      <circle className="word-cursor-dot" cx={BRAND_MARK.dot.cx} cy={BRAND_MARK.dot.cy} r={BRAND_MARK.dot.r} />
    </svg>
  );
}

export function HeroHeadline() {
  const lines = timeline();
  return (
    <h1 className="hero-title" style={{ "--release": seconds(RELEASE) } as CSSProperties}>
      {lines.map((line, l) => {
        const words = line.flatMap((word, w) => [
          w > 0 ? " " : null,
          <span
            key={word.text}
            className={`hero-word${word.last ? " is-last" : ""}`}
            style={{
              "--heard": seconds(word.at),
              "--off": seconds(word.last ? RELEASE + SETTLE : word.next),
              "--final": seconds(RELEASE + word.index * SWEEP),
            } as CSSProperties}
          >
            {/* O texto anima num filho próprio. Se a animação ficasse no span de
                fora, o cursor herdaria o desfoque e a opacidade da palavra
                provisória — e o ponto de gravação, o elemento mais nítido da
                tela, sairia borrado a 58%. */}
            <span className="hero-word-text">{word.text}</span>
            <Cursor last={word.last} />
          </span>,
        ]);
        // As linhas são blocos, e entre blocos o navegador não põe espaço no
        // nome acessível: o leitor de tela ouvia "palavras.No". O espaço vai
        // explícito; no fim de uma linha ele não ocupa lugar nenhum na tela.
        const gap = l < lines.length - 1 ? " " : null;
        return (
          <span key={l} className="hero-title-line">
            {LINES[l].emphasis ? <em>{words}</em> : words}
            {gap}
          </span>
        );
      })}
    </h1>
  );
}
