import assert from "node:assert/strict";
import test from "node:test";

import { MAX_NOTES, firstSentence, notesFromBody, stripMarkdown } from "./release-notes.mjs";

/**
 * Os corpos abaixo são releases de verdade do ISPer, copiadas da API. O parser
 * lê texto livre escrito por gente, então um teste sobre um formato inventado
 * prova pouco: o que quebra na prática é a pontuação real — número de versão
 * com ponto, `m.id`, continuação em três linhas, sub-item indentado.
 */

const V0_17_1 = `### Corrigido
- **O instalador da 0.17.0 fechava o app na primeira transcrição** (ditado ou
  reunião) em CPUs sem AVX-512, com instrução ilegal (\`0xc000001d\`). O
  whisper.cpp compilava com \`GGML_NATIVE\`, que otimiza para a CPU de quem
  compila — e o runner do GitHub daquela release tinha AVX-512.`;

const V0_16_1 = `### Corrigido
- **A Biblioteca ordenava por ordem de inserção, não pela data da reunião**
  (\`ORDER BY m.id DESC\`). Enquanto toda reunião entrava na ordem em que
  acontecia, os dois coincidiam. Agora ordena por \`started_ts\`.

### Adicionado
- **Reimportar reuniões da pasta** (Configurações → Sistema, e
  \`isper-cli import\`). O \`.md\` em \`Documentos\\ISPer\\Reunioes\` é gravado
  ANTES do banco e sobrevive a qualquer acidente com o índice.

  O que NÃO volta é a granularidade original: o Markdown guarda parágrafos.`;

const ANINHADO = `### Adicionado
- Fase 7.3 (segurança e confiança do binário), primeira parte:
  - **Release no GitHub Actions** (\`release.yml\`): as duas variantes do
    instalador compiladas em runners do GitHub, SBOM CycloneDX.
  - **SBOM** e **somas SHA-256** publicados com cada release.
- Fase 7.4 (dados e observabilidade responsável):
  - **Retenção** configurável.`;

test("junta as continuações e para no fim da primeira frase", () => {
  assert.deepEqual(notesFromBody(V0_17_1), [
    "Corrigido — O instalador da 0.17.0 fechava o app na primeira transcrição "
      + "(ditado ou reunião) em CPUs sem AVX-512, com instrução ilegal (0xc000001d).",
  ]);
});

test("o ponto de um número de versão não termina a frase", () => {
  // "0.17.0" tem dois pontos, e nenhum deles encerra nada.
  const nota = notesFromBody(V0_17_1)[0];
  assert.ok(nota.includes("da 0.17.0 fechava"), nota);
  assert.ok(nota.endsWith("(0xc000001d)."), nota);
});

test("cada seção etiqueta as próprias notas", () => {
  assert.deepEqual(notesFromBody(V0_16_1), [
    "Corrigido — A Biblioteca ordenava por ordem de inserção, não pela data da reunião (ORDER BY m.id DESC).",
    "Adicionado — Reimportar reuniões da pasta (Configurações → Sistema, e isper-cli import).",
  ]);
});

test("um parágrafo solto depois de linha em branco não entra na nota", () => {
  // A ressalva "O que NÃO volta…" pertence ao CHANGELOG, não à página.
  assert.ok(!notesFromBody(V0_16_1).some((nota) => nota.includes("granularidade")));
});

test("sub-item detalha o pai, então o pai vira a nota e ele não", () => {
  assert.deepEqual(notesFromBody(ANINHADO), [
    "Adicionado — Fase 7.3 (segurança e confiança do binário), primeira parte",
    "Adicionado — Fase 7.4 (dados e observabilidade responsável)",
  ]);
});

test("os dois-pontos de quem apresenta uma lista saem do fim", () => {
  // A lista que eles anunciavam não vem junto; deixá-los é prometer o que falta.
  for (const nota of notesFromBody(ANINHADO)) assert.ok(!nota.endsWith(":"), nota);
});

test("corpo com CRLF se comporta igual", () => {
  // A API do GitHub devolve o corpo com \r\n.
  assert.deepEqual(notesFromBody(V0_16_1.replace(/\n/g, "\r\n")), notesFromBody(V0_16_1));
});

test("marcação some, porque a lista renderiza texto puro", () => {
  assert.equal(stripMarkdown("**negrito** e \`código\`"), "negrito e código");
  assert.equal(stripMarkdown("veja o [guia](https://exemplo/x)"), "veja o guia");
  assert.equal(stripMarkdown("quebra\n   em    linhas"), "quebra em linhas");
});

test("frase curta demais leva a seguinte junto", () => {
  // Sozinha, "Corrigido." não diz o que foi corrigido.
  assert.equal(
    firstSentence("Corrigido. O atalho global parava de responder depois de suspender a máquina."),
    "Corrigido. O atalho global parava de responder depois de suspender a máquina.",
  );
});

test("texto sem ponto final volta inteiro", () => {
  assert.equal(firstSentence("Uma nota sem pontuação nenhuma"), "Uma nota sem pontuação nenhuma");
});

test("asterisco também marca item", () => {
  assert.deepEqual(notesFromBody("### Alterado\n* Trocamos o modelo padrão para o large-v3-turbo."), [
    "Alterado — Trocamos o modelo padrão para o large-v3-turbo.",
  ]);
});

test("item sem seção fica sem etiqueta", () => {
  assert.deepEqual(notesFromBody("- Uma mudança solta, sem cabeçalho de seção acima dela."), [
    "Uma mudança solta, sem cabeçalho de seção acima dela.",
  ]);
});

test("a página não vira um segundo CHANGELOG", () => {
  const muitos = ["### Adicionado"]
    .concat(Array.from({ length: 9 }, (_, i) => `- Mudança número ${i} desta release, descrita por extenso.`))
    .join("\n");
  assert.equal(notesFromBody(muitos).length, MAX_NOTES);
});

test("corpo vazio não inventa nota", () => {
  // O sync trata isso como erro; aqui o contrato é só não devolver lixo.
  for (const vazio of ["", "   ", null, undefined, "### Corrigido\n\nsem itens aqui"]) {
    assert.deepEqual(notesFromBody(vazio), [], JSON.stringify(vazio));
  }
});
