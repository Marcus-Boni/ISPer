/**
 * Notas da página de download a partir do corpo de uma release.
 *
 * O corpo segue o Keep a Changelog, como o `scripts/release.ps1` o monta:
 * seções `### Adicionado` / `### Corrigido` e itens `- **resumo** detalhe…`,
 * com as continuações indentadas. O negrito é o resumo que o autor escreveu,
 * e a primeira frase costuma ser exatamente a nota que cabe numa lista —
 * o resto é para quem abre o CHANGELOG.
 *
 * Puro de propósito: sem rede e sem disco, para poder ser testado com
 * corpos de release de verdade. Quem busca e grava é o sync-release.mjs.
 */

/** Quantos itens a página mostra antes de virar um segundo CHANGELOG. */
export const MAX_NOTES = 4;

/** Abaixo disto uma frase não diz nada sozinha, e leva a seguinte junto. */
const MIN_SENTENCE = 60;

/**
 * Markdown para texto corrido. A lista renderiza texto puro, então deixar
 * `**` ou backtick passar significa mostrar a marcação para quem lê.
 */
export function stripMarkdown(text) {
  return text
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Corta na primeira quebra de frase — mas só onde há frase. Um ponto seguido
 * de espaço termina algo; o ponto de "0.17.0" ou de "m.id" não. Se a primeira
 * frase for curta demais para significar alguma coisa, leva a seguinte.
 */
export function firstSentence(text) {
  let cut = 0;
  while (cut < text.length) {
    const next = text.slice(cut).search(/[.!?](\s|$)/);
    if (next === -1) return text;
    cut += next + 1;
    if (cut >= MIN_SENTENCE) return text.slice(0, cut).trim();
  }
  return text;
}

/**
 * Itens de primeiro nível, com a seção na frente.
 *
 * Sub-itens são ignorados, e não colados no pai: no CHANGELOG deste projeto
 * eles detalham um tema que a linha de cima já nomeia ("Fase 7.3…, primeira
 * parte:"), então o pai é o resumo e os filhos são o corpo. Antes de existir
 * este tratamento, um item virava o pai e todos os filhos numa frase só.
 *
 * Os dois-pontos de quem apresenta uma lista saem do fim da nota: a lista que
 * eles anunciavam não vem junto.
 */
export function notesFromBody(body) {
  const lines = String(body ?? "").split(/\r?\n/);
  const items = [];
  let section = null;
  let current = null;

  const flush = () => {
    if (!current) return;
    const text = stripMarkdown(current.text);
    if (text) items.push({ section: current.section, text });
    current = null;
  };

  for (const line of lines) {
    const heading = line.match(/^#{1,6}\s+(.*)$/);
    if (heading) {
      flush();
      section = stripMarkdown(heading[1]);
      continue;
    }
    const bullet = line.match(/^ {0,1}[-*]\s+(.*)$/);
    if (bullet) {
      flush();
      current = { section, text: bullet[1] };
      continue;
    }
    // Sub-item: encerra o item de cima e não entra em nota nenhuma. As
    // continuações dele caem no `current` nulo logo abaixo e somem junto.
    if (/^\s{2,}[-*]\s/.test(line)) {
      flush();
      continue;
    }
    if (current && /^\s{2,}\S/.test(line)) {
      current.text += ` ${line.trim()}`;
      continue;
    }
    if (!line.trim()) flush();
  }
  flush();

  return items.slice(0, MAX_NOTES).map(({ section: label, text }) => {
    const sentence = firstSentence(text).replace(/:$/, "");
    return label ? `${label} — ${sentence}` : sentence;
  });
}
