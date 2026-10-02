// Avalia uma expressão JS numa tela do ISPer via Chrome DevTools Protocol.
//
//   node cdp.mjs <trecho-da-url | id> "<expressão>"
//   CDP_EXPR="<expressão>" node cdp.mjs <trecho-da-url | id>
//   node cdp.mjs --list          (as URLs de todas as telas abertas, em JSON)
//
// A tela pode ser uma janela (o indicador, o Copilot) ou um iframe da janela
// principal (Início, Biblioteca, Configurações, primeira configuração) — ver
// cdp-target.mjs. A forma por variável de ambiente existe porque o
// PowerShell 5.1 não escapa aspas duplas ao montar a linha de comando de um
// exe — uma expressão com querySelector(".x") chegaria sem as aspas. O
// common.ps1 usa sempre CDP_EXPR. O app precisa ter sido aberto com
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223
// (common.ps1 faz isso). `await` só dentro de uma IIFE assíncrona:
// (async () => { ... })(). Requer Node 22+ (WebSocket global).
import { attach, listUrls, notFound } from './cdp-target.mjs';

const [, , match, argExpr] = process.argv;
if (match === '--list') {
  console.log(JSON.stringify(await listUrls()));
  process.exit(0);
}
const expr = argExpr ?? process.env.CDP_EXPR;
if (!expr) {
  console.log('faltou a expressão (argumento ou CDP_EXPR)');
  process.exit(1);
}
const t = await attach(match);
if (!t) {
  console.log(await notFound());
  process.exit(1);
}
// unref(): sem isso o timer segura o processo por 8 s mesmo com a resposta em mãos.
const m = await Promise.race([t.evaluate(expr), new Promise((res) => setTimeout(() => res({ timeout: true }), 8000).unref())]);
if (m.timeout) console.log('TIMEOUT (sem resposta em 8 s)');
else if (m.result?.exceptionDetails) console.log('EXCEPTION: ' + JSON.stringify(m.result.exceptionDetails.exception?.description || m.result.exceptionDetails));
else console.log(JSON.stringify(m.result?.result?.value ?? m.result?.result ?? m.error));
// Sai sozinho depois de fechar a conexão: process.exit() com o socket ainda
// fechando dispara uma asserção do libuv no Windows (UV_HANDLE_CLOSING).
await t.close();
