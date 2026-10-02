// Acha uma tela do ISPer no Chrome DevTools Protocol, seja ela uma janela
// própria (o indicador, o Copilot) ou uma tela da janela principal — Início,
// Biblioteca, Configurações e a primeira configuração são iframes do
// app.html desde o ADR 0020. Para os scripts, as duas coisas se pedem igual:
// por um trecho da URL ('settings.html'), e a expressão roda no contexto da
// própria tela (o window dela, com o __TAURI__ que o boot.js liga à mãe).
//
//   import { attach, listUrls } from './cdp-target.mjs';
//   const t = await attach('library.html');   // { send, evaluate, frame, close }
//
// Requer Node 22+ (fetch e WebSocket globais).
const port = process.env.CDP_PORT || '9223';

async function targets() {
  return (await fetch(`http://127.0.0.1:${port}/json`)).json();
}

function connect(target) {
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  let next = 1;
  const pending = new Map();
  const listeners = [];
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); return; }
    for (const l of listeners) l(m);
  };
  // A tela fechou no meio (ex.: o teste fecha a janela): quem esperava resposta
  // recebe um erro em vez de ficar pendurado.
  ws.onclose = () => {
    for (const res of pending.values()) res({ error: { message: 'a tela fechou' } });
    pending.clear();
  };
  const ready = new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
  const closed = new Promise((res) => ws.addEventListener('close', res));
  // Fecha e espera a conexão terminar (com prazo): sair do processo com o
  // WebSocket ainda fechando dispara uma asserção do libuv no Windows.
  const close = () => {
    try { ws.close(); } catch (_) {}
    return Promise.race([closed, new Promise((r) => setTimeout(r, 500))]);
  };
  const send = (method, params = {}) => new Promise((res) => {
    const id = next++;
    pending.set(id, res);
    ws.send(JSON.stringify({ id, method, params }));
  });
  return { ws, ready, send, close, on: (fn) => listeners.push(fn) };
}

function walk(tree, out = []) {
  out.push(tree.frame);
  for (const c of tree.childFrames || []) walk(c, out);
  return out;
}

// As URLs de todas as telas abertas: janelas e iframes da janela principal.
export async function listUrls() {
  const all = await targets();
  const urls = [];
  for (const t of all.filter((x) => x.type === 'page')) {
    urls.push(t.url);
    if (!t.url.includes('app.html')) continue;
    const c = connect(t);
    await c.ready;
    const tree = await c.send('Page.getFrameTree');
    for (const f of walk(tree.result.frameTree).slice(1)) urls.push(f.url);
    await c.close();
  }
  return urls;
}

export async function attach(match) {
  const all = await targets();
  const pages = all.filter((x) => x.type === 'page');
  const page =
    pages.find((x) => x.url === match) ||
    pages.find((x) => x.id === match || x.url.includes(match));
  if (page) {
    const c = connect(page);
    await c.ready;
    return {
      send: c.send,
      frame: null,
      evaluate: (expression, extra = {}) => c.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, ...extra }),
      close: c.close,
    };
  }
  // Uma tela dentro da janela principal: o contexto de execução do iframe.
  const shell = pages.find((x) => x.url.includes('app.html'));
  if (!shell) return null;
  const c = connect(shell);
  await c.ready;
  const tree = await c.send('Page.getFrameTree');
  const frame = walk(tree.result.frameTree).slice(1).find((f) => f.url.includes(match));
  if (!frame) { await c.close(); return null; }
  const contexts = new Map();
  c.on((m) => {
    if (m.method === 'Runtime.executionContextCreated') {
      const ctx = m.params.context;
      if (ctx.auxData && ctx.auxData.isDefault) contexts.set(ctx.auxData.frameId, ctx.id);
    }
  });
  await c.send('Runtime.enable');
  for (let i = 0; i < 40 && !contexts.has(frame.id); i++) await new Promise((r) => setTimeout(r, 50));
  const contextId = contexts.get(frame.id);
  if (!contextId) { await c.close(); return null; }
  return {
    send: c.send,
    // A tela é um iframe: data-view no app.html (home, library…).
    frame: { id: frame.id, url: frame.url, view: (frame.url.match(/([a-z]+)\.html/) || [])[1] },
    evaluate: (expression, extra = {}) => c.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, contextId, ...extra }),
    // Põe a tela à vista na janela principal (teclado e captura de tela precisam dela visível).
    show: async () => {
      const view = (frame.url.match(/([a-z]+)\.html/) || [])[1];
      await c.send('Runtime.evaluate', { expression: `window.ISPER_SHELL && window.ISPER_SHELL.show(${JSON.stringify(view)})`, awaitPromise: true });
      await new Promise((r) => setTimeout(r, 450));
    },
    close: c.close,
  };
}

export async function notFound() {
  try { return 'alvo não encontrado; existem: ' + (await listUrls()).join(' , '); } catch (e) { return 'alvo não encontrado (' + e.message + ')'; }
}
