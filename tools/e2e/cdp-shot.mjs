// Captura a tela de uma janela do ISPer via Chrome DevTools Protocol (PNG).
// Uma tela da janela principal (home.html, library.html…) é posta à vista
// e a captura mostra a janela inteira.
//
//   node cdp-shot.mjs <trecho-da-url | id> <saida.png>
//
// Mesma pré-condição do cdp.mjs: o app aberto com a porta CDP (common.ps1 →
// Start-Isper). Serve para conferir à vista o que os testes medem por número
// — tema claro/escuro, textos traduzidos, o onboarding — e para anexar ao PR.
import { writeFileSync } from 'node:fs';
import { attach, notFound } from './cdp-target.mjs';

const [, , match, out] = process.argv;
if (!match || !out) {
  console.log('uso: node cdp-shot.mjs <trecho-da-url | id> <saida.png>');
  process.exit(1);
}
const t = await attach(match);
if (!t) {
  console.log(await notFound());
  process.exit(1);
}
// Uma tela da janela principal: vai à vista antes (a captura é da janela
// inteira, com a barra lateral — é o que o usuário vê).
if (t.show) await t.show();
const m = await Promise.race([t.send('Page.captureScreenshot', { format: 'png' }), new Promise((res) => setTimeout(() => res({ timeout: true }), 10000).unref())]);
if (m.timeout || !m.result?.data) {
  console.log('falhou: ' + JSON.stringify(m.error || m));
  process.exit(1);
}
writeFileSync(out, Buffer.from(m.result.data, 'base64'));
console.log('ok ' + out);
// Sai sozinho depois de fechar a conexão: process.exit() com o socket ainda
// fechando dispara uma asserção do libuv no Windows (UV_HANDLE_CLOSING).
await t.close();
