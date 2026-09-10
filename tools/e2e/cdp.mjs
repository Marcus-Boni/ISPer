// Avalia uma expressão JS numa janela do ISPer via Chrome DevTools Protocol.
//
//   node cdp.mjs <trecho-da-url | id> "<expressão>"
//   CDP_EXPR="<expressão>" node cdp.mjs <trecho-da-url | id>
//
// A forma por variável de ambiente existe porque o PowerShell 5.1 não escapa
// aspas duplas ao montar a linha de comando de um exe — uma expressão com
// querySelector(".x") chegaria sem as aspas. O common.ps1 usa sempre CDP_EXPR.
// O app precisa ter sido aberto com WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=
// --remote-debugging-port=9223 (common.ps1 faz isso). `await` só dentro de uma
// IIFE assíncrona: (async () => { ... })(). Requer Node 22+ (WebSocket global).
const [, , match, argExpr] = process.argv;
const expr = argExpr ?? process.env.CDP_EXPR;
if (!expr) {
  console.log('faltou a expressão (argumento ou CDP_EXPR)');
  process.exit(1);
}
const port = process.env.CDP_PORT || '9223';
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const t =
  targets.find((x) => x.type === 'page' && x.url === match) ||
  targets.find((x) => x.type === 'page' && (x.id === match || x.url.includes(match)));
if (!t) {
  console.log('alvo não encontrado; existem: ' + targets.map((x) => x.url).join(' , '));
  process.exit(1);
}
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
const reply = new Promise((res) => {
  ws.onmessage = (e) => { const m = JSON.parse(e.data); if (m.id === 1) res(m); };
});
ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression: expr, awaitPromise: true, returnByValue: true } }));
// unref(): sem isso o timer segura o processo por 8 s mesmo com a resposta em mãos.
const m = await Promise.race([reply, new Promise((res) => setTimeout(() => res({ timeout: true }), 8000).unref())]);
if (m.timeout) console.log('TIMEOUT (sem resposta em 8 s)');
else if (m.result?.exceptionDetails) console.log('EXCEPTION: ' + JSON.stringify(m.result.exceptionDetails.exception?.description || m.result.exceptionDetails));
else console.log(JSON.stringify(m.result?.result?.value ?? m.result?.result ?? m.error));
ws.close();
process.exit(0);
