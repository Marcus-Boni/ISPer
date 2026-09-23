// Captura a tela de uma janela do ISPer via Chrome DevTools Protocol (PNG).
//
//   node cdp-shot.mjs <trecho-da-url | id> <saida.png>
//
// Mesma pré-condição do cdp.mjs: o app aberto com a porta CDP (common.ps1 →
// Start-Isper). Serve para conferir à vista o que os testes medem por número
// — tema claro/escuro, textos traduzidos, o onboarding — e para anexar ao PR.
import { writeFileSync } from 'node:fs';

const [, , match, out] = process.argv;
if (!match || !out) {
  console.log('uso: node cdp-shot.mjs <trecho-da-url | id> <saida.png>');
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
ws.send(JSON.stringify({ id: 1, method: 'Page.captureScreenshot', params: { format: 'png' } }));
const m = await Promise.race([reply, new Promise((res) => setTimeout(() => res({ timeout: true }), 10000).unref())]);
if (m.timeout || !m.result?.data) {
  console.log('falhou: ' + JSON.stringify(m.error || m));
  process.exit(1);
}
writeFileSync(out, Buffer.from(m.result.data, 'base64'));
console.log('ok ' + out);
ws.close();
process.exit(0);
