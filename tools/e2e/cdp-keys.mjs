// Aperta teclas DE VERDADE numa janela do ISPer via Chrome DevTools Protocol
// (Input.dispatchKeyEvent: o navegador trata como teclado físico, com
// :focus-visible e ordem de Tab reais) e, depois de cada tecla, chama
// window.__a11yStep() se a página tiver um — é assim que o a11y.ps1 registra
// por onde o foco passou.
//
//   node cdp-keys.mjs <trecho-da-url> <vezes> [Tab|ShiftTab|Enter|Escape|ArrowRight|ArrowLeft|Space]
//   (CDP_MEDIA=forced-colors:active emula um tema de contraste do Windows)
//
// Requer Node 22+ (WebSocket global) e o app aberto com a porta CDP (common.ps1).
const [, , match, timesArg, keyArg = 'Tab'] = process.argv;
const times = Math.max(1, Number(timesArg) || 1);
const port = process.env.CDP_PORT || '9223';
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const t =
  targets.find((x) => x.type === 'page' && x.url === match) ||
  targets.find((x) => x.type === 'page' && x.url.includes(match));
if (!t) {
  console.log('alvo não encontrado; existem: ' + targets.map((x) => x.url).join(' , '));
  process.exit(1);
}
const KEYS = {
  Tab: { key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 },
  ShiftTab: { key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9, modifiers: 8 },
  Enter: { key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' },
  Escape: { key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 },
  ArrowRight: { key: 'ArrowRight', code: 'ArrowRight', windowsVirtualKeyCode: 39 },
  ArrowLeft: { key: 'ArrowLeft', code: 'ArrowLeft', windowsVirtualKeyCode: 37 },
  Space: { key: ' ', code: 'Space', windowsVirtualKeyCode: 32, text: ' ' },
};
const k = KEYS[keyArg];
if (!k) {
  console.log('tecla desconhecida: ' + keyArg);
  process.exit(1);
}
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let next = 1;
const pending = new Map();
ws.onmessage = (e) => {
  const m = JSON.parse(e.data);
  if (pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
};
const send = (method, params = {}) => new Promise((res) => {
  const id = next++;
  pending.set(id, res);
  ws.send(JSON.stringify({ id, method, params }));
});
const timer = setTimeout(() => { console.log('TIMEOUT'); process.exit(1); }, 30000);
// A janela pode não estar em primeiro plano: sem isto o foco não se move.
await send('Emulation.setFocusEmulationEnabled', { enabled: true });
// CDP_MEDIA=forced-colors:active emula um tema de contraste do Windows nesta
// sessão (vale enquanto as teclas são apertadas e o __a11yStep registra).
if (process.env.CDP_MEDIA) {
  const features = process.env.CDP_MEDIA.split(',').map((f) => {
    const [name, value] = f.split(':');
    return { name, value };
  });
  await send('Emulation.setEmulatedMedia', { features });
}
for (let i = 0; i < times; i++) {
  const { text, ...rest } = k;
  await send('Input.dispatchKeyEvent', { type: text ? 'keyDown' : 'rawKeyDown', ...rest, ...(text ? { text } : {}) });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', ...rest });
  await send('Runtime.evaluate', { expression: 'window.__a11yStep && window.__a11yStep()', awaitPromise: true });
}
clearTimeout(timer);
console.log(JSON.stringify({ pressed: times, key: keyArg }));
ws.close();
process.exit(0);
