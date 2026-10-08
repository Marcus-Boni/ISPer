// ISPer — a janela principal (app.html): barra lateral, telas vivas em
// iframes, paleta de comandos (Ctrl+K), atalhos de teclado, gravação de
// reunião sempre à mão e soltar áudio em qualquer tela.
//
// Como as telas conversam com o app: o Tauri só injeta o __TAURI__ nesta
// página (os scripts dele valem para o frame principal). O boot.js de cada
// tela, ao se ver dentro de um iframe, usa o __TAURI__ daqui — os comandos
// e eventos são os mesmos de quando cada tela era uma janela. Pedidos de
// "abrir tela X" vindos do Rust (bandeja, notificação, botão de outra tela)
// chegam pelo evento `isper-nav`.
//
// O que uma tela pode oferecer à janela (opcional), em window.ISPER_VIEW:
//   open(params)  — chamada depois de mostrar a tela com parâmetros
//                   ({ section }, { tab }, { drop: [caminhos] }, { import: true }…)
//   save()        — Configurações: salva o que está pendente
// E o que a janela oferece às telas, em window.ISPER_SHELL (o boot.js
// repassa): key(evento) e mouse(botão) para os atalhos funcionarem com o foco
// dentro da tela, e setDirty(tela, bool) para o aviso de alterações não salvas.
(function () {
  'use strict';

  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);
  const { el, toast } = UI;
  const body = document.body;
  const stage = $('stage');

  const VIEWS = { home: 'home.html', today: 'today.html', library: 'library.html', settings: 'settings.html', onboarding: 'onboarding.html' };
  const NAMES = { home: 'shell.nav.home', today: 'shell.nav.today', library: 'shell.nav.library', settings: 'shell.nav.settings', onboarding: 'window.onboarding' };
  // Seções das Configurações (id do card em settings.html → título).
  const SECTIONS = [
    ['aparencia', 'settings.aparencia.titulo'],
    ['ditado', 'settings.ditado.ditado'],
    ['modelos', 'settings.modelos-whisper.modelos-whisper'],
    ['reunioes', 'settings.reunioes.reunioes'],
    ['celular', 'settings.celular.titulo'],
    ['inteligencia', 'settings.inteligencia.inteligencia'],
    ['sistema', 'settings.sistema.sistema'],
  ];
  const boot = window.__ISPER_SHELL || { route: { view: 'home' } };

  // As telas criadas depois de uma troca de idioma precisam nascer com o
  // dicionário novo: o boot.js delas lê window.__ISPER_UI daqui.
  listen('isper-ui', (e) => { if (e.payload) window.__ISPER_UI = e.payload; });

  // ------------------------------------------------------------------ telas
  const frames = new Map();      // tela → { el, ready: Promise<Window>, loaded }
  let current = null;
  const hist = [];
  let hpos = -1;
  const dirty = { settings: false };
  let waitTimer = null;

  function frameFor(view) {
    let f = frames.get(view);
    if (f) return f;
    const node = document.createElement('iframe');
    node.className = 'view loading';
    node.dataset.view = view;
    node.title = t(NAMES[view]);
    // Sem tabIndex = -1: num iframe, ele tira a tela inteira da ordem do Tab
    // (o teclado não passaria da barra lateral). As telas escondidas já saem
    // da ordem pelo inert e pelo visibility: hidden.
    node.setAttribute('inert', '');
    node.setAttribute('aria-hidden', 'true');
    let resolve;
    f = { el: node, ready: new Promise((r) => { resolve = r; }), loaded: false };
    node.addEventListener('load', () => {
      f.loaded = true;
      node.classList.remove('loading');
      tellActive(view);
      if (view === current) hideWait();
      resolve(node.contentWindow);
    }, { once: true });
    node.src = VIEWS[view];
    stage.appendChild(node);
    frames.set(view, f);
    return f;
  }

  function setOn(f, on) {
    f.el.classList.toggle('on', on);
    f.el.toggleAttribute('inert', !on);
    if (on) f.el.removeAttribute('aria-hidden'); else f.el.setAttribute('aria-hidden', 'true');
  }

  // A tela sabe se está à vista (a Biblioteca só trata o arrastar quando está).
  function tellActive(view) {
    const f = frames.get(view);
    if (!f || !f.loaded) return;
    try {
      const w = f.el.contentWindow;
      w.__isperActive = view === current;
      w.document.dispatchEvent(new w.CustomEvent('isper-view', { detail: { active: view === current } }));
    } catch (_) {}
  }

  function showWait() {
    clearTimeout(waitTimer);
    waitTimer = setTimeout(() => { $('stage-wait').hidden = false; }, 160);
  }
  function hideWait() { clearTimeout(waitTimer); $('stage-wait').hidden = true; }

  async function show(view, params, opts = {}) {
    if (!VIEWS[view]) return;
    // Durante a primeira configuração, a janela só sai dela por ela mesma
    // (Concluir/Pular) ou por um pedido do app.
    if (current === 'onboarding' && view !== 'onboarding' && !opts.fromRust && !opts.force) return;
    const prev = current;
    if (view !== prev) {
      if (prev === 'settings' && dirty.settings) remindUnsaved();
      current = view;
      body.dataset.view = view;
      const f = frameFor(view);
      for (const [v, other] of frames) setOn(other, v === view);
      if (!f.loaded) showWait(); else hideWait();
      paintNav();
      if (opts.push !== false) { hist.splice(hpos + 1); hist.push(view); hpos = hist.length - 1; }
      if (!opts.fromRust) invoke('shell_view', { view }).catch(() => {});
      announce(t(NAMES[view]));
      for (const v of frames.keys()) tellActive(v);
      // A primeira configuração recomeça do passo 1 se for reaberta.
      if (prev === 'onboarding' && !opts.keep) setTimeout(() => dropFrame('onboarding'), 400);
    }
    const f = frames.get(view);
    const win = await f.ready;
    if (current !== view) return;
    if (params && win.ISPER_VIEW && typeof win.ISPER_VIEW.open === 'function') {
      try { win.ISPER_VIEW.open(params); } catch (e) { console.error(e); }
    }
    if (view !== prev && opts.focus !== false) {
      try { f.el.focus(); } catch (_) {}
    }
  }

  function dropFrame(view) {
    const f = frames.get(view);
    if (!f || view === current) return;
    f.el.remove();
    frames.delete(view);
  }

  function back() { if (hpos > 0) { hpos -= 1; show(hist[hpos], null, { push: false }); } }
  function forward() { if (hpos < hist.length - 1) { hpos += 1; show(hist[hpos], null, { push: false }); } }

  function announce(text) {
    const a = $('announce');
    a.textContent = '';
    requestAnimationFrame(() => { a.textContent = text; });
  }

  function remindUnsaved() {
    toast(t('shell.unsaved-toast'), 'info', 7000, {
      action: { label: t('shell.unsaved-save'), run: () => withView('settings', (w) => w.ISPER_VIEW && w.ISPER_VIEW.save && w.ISPER_VIEW.save()) },
    });
  }

  function withView(view, fn) {
    const f = frames.get(view);
    if (f) f.ready.then((w) => { try { fn(w); } catch (e) { console.error(e); } });
  }

  // Pré-carrega uma tela quando o mouse passa no item dela: o clique abre já pronta.
  function prefetch(view) { if (VIEWS[view] && view !== 'onboarding') frameFor(view); }

  // -------------------------------------------------------- barra lateral
  const navItems = [...document.querySelectorAll('.nav-item[data-view]')];
  const SHORTCUT_HINT = { home: 'Ctrl+1', today: 'Ctrl+T', library: 'Ctrl+2', settings: 'Ctrl+,' };

  function paintNav() {
    for (const b of navItems) {
      const on = b.dataset.view === current;
      if (on) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current');
      const name = t(NAMES[b.dataset.view]);
      b.title = name + ' (' + SHORTCUT_HINT[b.dataset.view] + ')';
      b.setAttribute('aria-label', name);
    }
    const cop = $('nav-copilot');
    cop.title = t('shell.nav.copilot-hint') + ' (Ctrl+3)';
    cop.setAttribute('aria-label', t('shell.nav.copilot') + ' — ' + t('shell.nav.copilot-hint'));
    const ind = $('nav-ind');
    const active = navItems.find((b) => b.dataset.view === current && b.parentElement === $('nav'));
    if (active) {
      ind.style.setProperty('--y', (active.offsetTop + (active.offsetHeight - 18) / 2) + 'px');
      ind.classList.add('on');
    } else ind.classList.remove('on');
    for (const [v, f] of frames) f.el.title = t(NAMES[v]);
  }

  for (const b of navItems) {
    b.addEventListener('click', () => show(b.dataset.view));
    b.addEventListener('pointerenter', () => prefetch(b.dataset.view));
    b.addEventListener('focus', () => prefetch(b.dataset.view));
  }
  $('brand').addEventListener('click', () => show('home'));
  $('nav-copilot').addEventListener('click', () => invoke('open_copilot_window').catch((e) => toast(String(e), 'err')));
  $('open-palette').addEventListener('click', () => openPalette());
  $('keys').addEventListener('click', () => openKeys());
  $('update').addEventListener('click', () => show('home'));
  UI.themeToggle($('theme'));

  // Recolher: lembrado no config.toml. Janela estreita: trilho sozinho.
  const narrow = matchMedia('(max-width: 1060px)');
  let collapsed = !!boot.sidebar_collapsed;
  function paintRail() {
    body.classList.toggle('narrow', narrow.matches);
    body.classList.toggle('rail', collapsed || narrow.matches);
    const btn = $('collapse');
    const label = t(collapsed ? 'shell.expand' : 'shell.collapse') + ' (Ctrl+B)';
    btn.title = label;
    btn.setAttribute('aria-label', label);
    btn.setAttribute('aria-expanded', String(!collapsed));
    // No trilho, o nome de cada botão aparece no tooltip.
    const rail = body.classList.contains('rail');
    $('open-palette').title = rail ? t('shell.search') + ' (Ctrl+K)' : '';
    $('rec-start').title = rail ? t('shell.rec.start') : '';
    requestAnimationFrame(paintNav);
  }
  function toggleRail() {
    if (narrow.matches) return;
    collapsed = !collapsed;
    paintRail();
    invoke('set_sidebar_collapsed', { collapsed }).catch(() => {});
  }
  $('collapse').addEventListener('click', toggleRail);
  narrow.addEventListener('change', paintRail);

  // ---------------------------------------------------------------- gravação
  let status = null;
  let busy = null;          // texto da fase pós-reunião, ou null
  let startedAt = 0;
  let ticker = null;

  async function refreshStatus() {
    try { status = await invoke('shell_status'); } catch (_) { return; }
    paintStatus();
  }

  function paintStatus() {
    if (!status) return;
    $('ver').textContent = 'v' + status.version;
    $('rec-key').textContent = status.meeting_shortcut ? status.meeting_shortcut.replace(/\+/g, ' ') : '';
    const rec = $('rec');
    let state = 'idle';
    if (status.meeting_active) state = 'meeting';
    else if (busy || status.processing) state = 'busy';
    rec.dataset.state = state;
    $('rec-busy-text').textContent = busy || t('shell.rec.speakers');
    clearInterval(ticker);
    if (state === 'meeting') {
      startedAt = Date.now() - (status.meeting_elapsed_secs || 0) * 1000;
      const paint = () => { $('rec-timer').textContent = UI.fmtClock((Date.now() - startedAt) / 1000); };
      paint();
      ticker = setInterval(paint, 1000);
    }
    $('rec-mark').title = t('shell.rec.mark-hint') + (status.mark_shortcut ? ' (' + status.mark_shortcut + ')' : '');
    $('rec-mark').setAttribute('aria-label', t('shell.rec.mark-hint'));
    $('rec-stop').setAttribute('aria-label', t('shell.rec.stop-hint'));
    $('badge-copilot').hidden = !status.meeting_active;
    const up = $('update');
    up.hidden = !status.update;
    if (status.update) {
      const text = t('shell.update', { v: status.update });
      $('update-text').textContent = text;
      up.title = text;
    }
  }

  async function toggleMeeting() {
    try { await invoke('toggle_meeting_cmd'); } catch (e) { toast(t('common.error', { e }), 'err'); }
    refreshStatus();
  }
  async function markMoment() {
    try { await invoke('mark_moment_cmd'); } catch (e) { toast(t('common.error', { e }), 'err'); }
  }
  $('rec-start').addEventListener('click', toggleMeeting);
  $('rec-stop').addEventListener('click', toggleMeeting);
  $('rec-mark').addEventListener('click', markMoment);

  listen('isper-status', refreshStatus);
  listen('isper-state', (e) => {
    const s = e.payload && e.payload.state;
    if (s === 'meeting-processing') busy = t('shell.rec.processing');
    else if (s === 'meeting-summary') busy = t('shell.rec.summary');
    else if (s === 'meeting-done' || s === 'error' || s === 'idle' || s === 'meeting') busy = null;
    else return;
    if (status) paintStatus();
    refreshStatus();
  });
  // O Início já avisa quando está à vista; nas outras telas, quem avisa é a janela.
  listen('isper-moment', () => { if (current !== 'home') toast(t('shell.rec.marked'), 'ok', 1600); });
  listen('isper-import', (e) => { $('badge-library').hidden = !(e.payload && e.payload.current); });
  invoke('import_status').then((s) => { $('badge-library').hidden = !(s && s.current); }).catch(() => {});
  // Quantas tarefas são para hoje: relido quando elas mudam e na virada do dia.
  function paintTodayBadge() {
    invoke('today_badge').then((n) => {
      const b = $('badge-today');
      b.textContent = n > 0 ? String(n) : '';
      b.hidden = !(n > 0);
    }).catch(() => {});
  }
  listen('isper-tasks', paintTodayBadge);
  paintTodayBadge();
  let badgeDay = new Date().toDateString();
  setInterval(() => { const d = new Date().toDateString(); if (d !== badgeDay) { badgeDay = d; paintTodayBadge(); } }, 60_000);

  // ------------------------------------------------- soltar áudio em qualquer tela
  // Na Biblioteca, quem cuida é ela (área de soltar e fila próprias). Nas
  // outras telas, a janela mostra a área e leva os arquivos até a Biblioteca.
  const drop = $('dropzone');
  const ownsDrop = () => current !== 'library' && current !== 'onboarding';
  listen('tauri://drag-enter', () => { if (ownsDrop()) drop.hidden = false; });
  listen('tauri://drag-leave', () => { drop.hidden = true; });
  listen('tauri://drag-drop', (e) => {
    const mine = !drop.hidden;
    drop.hidden = true;
    const paths = (e.payload && e.payload.paths) || [];
    if (mine && paths.length) show('library', { drop: paths });
  });

  // ------------------------------------------------------------- atalhos
  const ctrl = (e) => (e.ctrlKey || e.metaKey) && !e.altKey;
  const SHORTCUTS = [
    { keys: 'Ctrl+K', name: 'shell.keys.palette', test: (e) => ctrl(e) && !e.shiftKey && e.key.toLowerCase() === 'k', run: () => openPalette() },
    { keys: 'Ctrl+1', name: 'shell.nav.home', test: (e) => ctrl(e) && e.code === 'Digit1', run: () => show('home') },
    { keys: 'Ctrl+T', name: 'shell.nav.today', test: (e) => ctrl(e) && !e.shiftKey && e.key.toLowerCase() === 't', run: () => show('today') },
    { keys: 'Ctrl+2', name: 'shell.nav.library', test: (e) => ctrl(e) && e.code === 'Digit2', run: () => show('library') },
    { keys: 'Ctrl+3', name: 'shell.keys.copilot', test: (e) => ctrl(e) && e.code === 'Digit3', run: () => invoke('open_copilot_window').catch(() => {}) },
    { keys: 'Ctrl+,', name: 'shell.nav.settings', test: (e) => ctrl(e) && (e.key === ',' || e.code === 'Comma'), run: () => show('settings') },
    { keys: 'Ctrl+B', name: 'shell.keys.sidebar', test: (e) => ctrl(e) && !e.shiftKey && e.key.toLowerCase() === 'b', run: toggleRail },
    { keys: 'Alt+←', name: 'shell.keys.back', test: (e) => e.altKey && !e.ctrlKey && e.key === 'ArrowLeft', run: back },
    { keys: 'Alt+→', name: 'shell.keys.forward', test: (e) => e.altKey && !e.ctrlKey && e.key === 'ArrowRight', run: forward },
    { keys: 'Ctrl+/', name: 'shell.keys.title', test: (e) => ctrl(e) && (e.key === '/' || e.code === 'Slash' || e.code === 'IntlRo'), run: () => openKeys() },
  ];

  // Devolve true quando a tecla era um atalho da janela (quem chamou cancela o padrão).
  function handleKey(e) {
    if (current === 'onboarding') return false;
    if ($('palette').open || $('keys-dialog').open) return false;
    for (const s of SHORTCUTS) {
      if (s.test(e)) { s.run(); return true; }
    }
    return false;
  }
  document.addEventListener('keydown', (e) => { if (!e.defaultPrevented && handleKey(e)) e.preventDefault(); });
  // Botões laterais do mouse: voltar e avançar entre telas.
  function onMouse(button) { if (button === 3) back(); else if (button === 4) forward(); }
  addEventListener('mouseup', (e) => { if (e.button === 3 || e.button === 4) { e.preventDefault(); onMouse(e.button); } });

  window.ISPER_SHELL = {
    key: handleKey,
    mouse: onMouse,
    // Para os testes e2e (cdp-target.mjs): põe uma tela à vista sem mexer no
    // foco e sem descartar a primeira configuração, que o roteiro ainda usa.
    show: (view) => show(view, null, { force: true, focus: false, keep: true }),
    setDirty(view, on) {
      if (!(view in dirty)) return;
      dirty[view] = !!on;
      const b = $('badge-' + view);
      if (b) { b.hidden = !on; b.title = on ? t('shell.unsaved') : ''; }
    },
  };

  // ---------------------------------------------------------- tela de atalhos
  function keyChips(combo) {
    const dd = el('dd');
    for (const part of String(combo).split('+').filter(Boolean)) dd.appendChild(el('kbd', null, part));
    return dd;
  }
  function openKeys() {
    const win = $('keys-window');
    win.replaceChildren();
    for (const s of SHORTCUTS) { win.appendChild(el('dt', null, t(s.name))); win.appendChild(keyChips(s.keys)); }
    const glob = $('keys-global');
    glob.replaceChildren();
    const g = status || {};
    for (const [name, combo] of [['shell.keys.dictate', g.shortcut], ['shell.keys.meeting', g.meeting_shortcut], ['shell.keys.mark', g.mark_shortcut], ['shell.keys.copilot', g.copilot_shortcut]]) {
      glob.appendChild(el('dt', null, t(name)));
      glob.appendChild(combo ? keyChips(combo) : el('dd', 'muted', t('common.none')));
    }
    $('keys-dialog').showModal();
  }
  $('keys-close').addEventListener('click', () => $('keys-dialog').close());
  $('keys-dialog').addEventListener('click', (e) => { if (e.target === $('keys-dialog')) $('keys-dialog').close(); });

  // ------------------------------------------------------- paleta de comandos
  const pal = $('palette');
  const input = $('pal-input');
  const list = $('pal-list');
  let items = [];
  let shown = [];
  let sel = 0;
  let meetings = [];
  let searchSeq = 0;
  let searchTimer = null;

  const ICONS = {
    go: 'M6 3.5 10.5 8 6 12.5',
    home: 'M2.5 7.2 8 2.6l5.5 4.6M3.9 6.2v6.6a.9.9 0 0 0 .9.9h2.3V10.4h1.8v3.3h2.3a.9.9 0 0 0 .9-.9V6.2',
    book: 'M3 2.5h7.5A1.5 1.5 0 0 1 12 4v9.5H4.5A1.5 1.5 0 0 0 3 15zM3 12.5A1.5 1.5 0 0 1 4.5 11H12',
    gear: 'M2 4.5h12M2 11.5h12M4.1 4.5a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0-3.8 0M8.1 11.5a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0-3.8 0',
    rec: 'M8 4.2a3.8 3.8 0 1 0 0 7.6a3.8 3.8 0 1 0 0-7.6',
    star: 'm8 1.9 1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.7l-3.8 2 .7-4.3-3.1-3 4.3-.6L8 1.9Z',
    spark: 'M8 1.8 9.4 5.9 13.5 7.3 9.4 8.7 8 12.8 6.6 8.7 2.5 7.3 6.6 5.9 8 1.8Z',
    pill: 'M5 5.5h6a2.5 2.5 0 0 1 0 5H5a2.5 2.5 0 0 1 0-5Z',
    upload: 'M8 10.5V2.5M5 5.5l3-3 3 3M2.5 10v2.5a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1V10',
    folder: 'M2 4.5a1 1 0 0 1 1-1h3l1.5 1.5H13a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z',
    moon: 'M13.3 9.7A5.5 5.5 0 0 1 6.3 2.7a5.5 5.5 0 1 0 7 7Z',
    side: 'M2 2.5h12v11H2zM6.2 2.5v11',
    keys: 'M1.8 4h12.4v8H1.8zM5 9.4h6',
    refresh: 'M13 3.5v3h-3M3 12.5v-3h3M12.2 6.3A4.5 4.5 0 0 0 4 5.2M3.8 9.7A4.5 4.5 0 0 0 12 10.8',
    mic: 'M8 2.2a2 2 0 0 1 2 2v3.6a2 2 0 0 1-4 0V4.2a2 2 0 0 1 2-2ZM4.4 7.6a3.6 3.6 0 0 0 7.2 0M8 11.2v2.4',
    doc: 'M4 2h5l3 3v9H4zM9 2v3h3',
    search: 'M7 2.5a4.5 4.5 0 1 0 0 9a4.5 4.5 0 1 0 0-9M10.5 10.5 14 14',
    check: 'M8 2.2a5.8 5.8 0 1 0 0 11.6a5.8 5.8 0 1 0 0-11.6M5.4 8.1 7.2 9.9 10.7 6.2',
    plus: 'M8 3v10M3 8h10',
  };
  function icon(name) {
    const NS = 'http://www.w3.org/2000/svg';
    const svg = document.createElementNS(NS, 'svg');
    svg.setAttribute('viewBox', '0 0 16 16');
    svg.setAttribute('fill', 'none');
    svg.setAttribute('stroke', 'currentColor');
    svg.setAttribute('stroke-width', '1.4');
    svg.setAttribute('stroke-linecap', 'round');
    svg.setAttribute('stroke-linejoin', 'round');
    svg.setAttribute('aria-hidden', 'true');
    const p = document.createElementNS(NS, 'path');
    p.setAttribute('d', ICONS[name] || ICONS.go);
    svg.appendChild(p);
    return svg;
  }

  // Sem acento e em minúsculas: "configuracoes" acha "Configurações".
  const fold = (s) => String(s || '').normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase();

  function buildItems() {
    const live = status && status.meeting_active;
    const out = [
      { group: 'shell.palette.group.nav', icon: 'home', label: t('shell.nav.home'), hint: 'Ctrl+1', run: () => show('home') },
      { group: 'shell.palette.group.nav', icon: 'check', label: t('shell.nav.today'), hint: 'Ctrl+T', keywords: t('shell.cmd.today-keywords'), run: () => show('today') },
      { group: 'shell.palette.group.nav', icon: 'book', label: t('shell.cmd.meetings'), hint: 'Ctrl+2', run: () => show('library', { tab: 'meetings' }) },
      { group: 'shell.palette.group.nav', icon: 'mic', label: t('shell.cmd.dictations'), run: () => show('library', { tab: 'dictations' }) },
      { group: 'shell.palette.group.nav', icon: 'gear', label: t('shell.nav.settings'), hint: 'Ctrl+,', run: () => show('settings') },
      { group: 'shell.palette.group.nav', icon: 'spark', label: t('shell.cmd.copilot'), hint: 'Ctrl+3', run: () => invoke('open_copilot_window') },
      live
        ? { group: 'shell.palette.group.actions', icon: 'rec', label: t('shell.cmd.meeting-stop'), hint: status.meeting_shortcut, run: toggleMeeting }
        : { group: 'shell.palette.group.actions', icon: 'rec', label: t('shell.cmd.meeting-start'), hint: status && status.meeting_shortcut, run: toggleMeeting },
    ];
    if (live) out.push({ group: 'shell.palette.group.actions', icon: 'star', label: t('shell.cmd.mark'), hint: status.mark_shortcut, run: markMoment });
    out.push(
      { group: 'shell.palette.group.actions', icon: 'plus', label: t('shell.cmd.task-new'), keywords: t('shell.cmd.today-keywords'), run: () => show('today', { focusAdd: true }) },
      { group: 'shell.palette.group.actions', icon: 'upload', label: t('shell.cmd.import'), run: () => show('library', { import: true }) },
      { group: 'shell.palette.group.actions', icon: 'search', label: t('shell.cmd.search-library'), run: () => show('library', { focusSearch: true }) },
      { group: 'shell.palette.group.actions', icon: 'pill', label: t('shell.cmd.indicator'), run: () => invoke('overlay_toggle_pin').catch((e) => toast(String(e), 'err')) },
      { group: 'shell.palette.group.actions', icon: 'folder', label: t('shell.cmd.folder'), run: () => invoke('open_meetings_folder').catch((e) => toast(String(e), 'err')) },
      { group: 'shell.palette.group.actions', icon: 'moon', label: t('shell.cmd.theme'), run: () => $('theme').click() },
      { group: 'shell.palette.group.actions', icon: 'side', label: t('shell.cmd.sidebar'), hint: 'Ctrl+B', run: toggleRail },
      { group: 'shell.palette.group.actions', icon: 'keys', label: t('shell.keys.title'), hint: 'Ctrl+/', run: () => openKeys() },
      { group: 'shell.palette.group.actions', icon: 'refresh', label: t('shell.cmd.update'), run: () => show('settings', { section: 'sistema', checkUpdate: true }) },
      { group: 'shell.palette.group.actions', icon: 'doc', label: t('shell.cmd.onboarding'), run: () => invoke('open_onboarding_window') },
    );
    for (const [id, key] of SECTIONS) {
      out.push({ group: 'shell.palette.group.settings', icon: 'gear', label: t('shell.nav.settings') + ' › ' + t(key), run: () => show('settings', { section: id }) });
    }
    return out;
  }

  function score(item, tokens) {
    if (!tokens.length) return 1;
    const hay = fold(item.label + ' ' + (item.keywords || ''));
    let s = 0;
    for (const tk of tokens) {
      const i = hay.indexOf(tk);
      if (i < 0) return 0;
      s += i === 0 ? 3 : hay[i - 1] === ' ' ? 2 : 1;
    }
    return s;
  }

  // Destaca os trechos encontrados (os índices batem: tirar o acento de um
  // caractere composto não muda o comprimento do texto).
  function highlight(label, tokens) {
    const span = el('span', 'pi-text');
    const f = fold(label);
    const marks = new Array(label.length).fill(false);
    for (const tk of tokens) {
      const i = f.indexOf(tk);
      if (i >= 0) for (let k = i; k < i + tk.length && k < marks.length; k++) marks[k] = true;
    }
    let buf = '';
    let on = false;
    const flush = () => { if (!buf) return; span.appendChild(on ? el('mark', null, buf) : document.createTextNode(buf)); buf = ''; };
    for (let k = 0; k < label.length; k++) {
      if (marks[k] !== on) { flush(); on = marks[k]; }
      buf += label[k];
    }
    flush();
    return span;
  }

  function render() {
    const q = input.value.trim();
    const tokens = fold(q).split(/\s+/).filter(Boolean);
    const ranked = items
      .map((it, i) => ({ it, i, s: score(it, tokens) }))
      .filter((x) => x.s > 0);
    if (tokens.length) ranked.sort((a, b) => b.s - a.s || a.i - b.i);
    // Sem busca, as seções das Configurações ficam de fora (a lista seria longa).
    shown = ranked.map((x) => x.it).filter((it) => tokens.length || it.group !== 'shell.palette.group.settings');
    if (tokens.length) shown = shown.concat(quickTask(q), meetings);
    list.replaceChildren();
    if (!shown.length) {
      list.appendChild(el('div', 'pal-empty', t('shell.palette.empty', { q })));
      input.removeAttribute('aria-activedescendant');
      return;
    }
    sel = Math.min(sel, shown.length - 1);
    // Agrupado na ordem em que os grupos aparecem.
    const order = [];
    for (const it of shown) if (!order.includes(it.group)) order.push(it.group);
    const flat = [];
    for (const grp of order) {
      const head = el('div', 'pal-group', t(grp));
      head.setAttribute('role', 'presentation');
      list.appendChild(head);
      for (const it of shown.filter((x) => x.group === grp)) {
        const idx = flat.length;
        flat.push(it);
        const row = el('div', 'pal-item');
        row.id = 'pal-opt-' + idx;
        row.setAttribute('role', 'option');
        row.setAttribute('aria-selected', String(idx === sel));
        row.appendChild(icon(it.icon));
        row.appendChild(highlight(it.label, tokens));
        if (it.meta) row.appendChild(el('span', 'pi-meta', it.meta));
        if (it.hint) {
          const k = el('span', 'pi-meta');
          for (const part of String(it.hint).split('+')) k.appendChild(el('kbd', null, part));
          row.appendChild(k);
        }
        row.addEventListener('mousemove', () => { if (sel !== idx) { sel = idx; paintSel(); } });
        row.addEventListener('click', () => runAt(idx));
        list.appendChild(row);
      }
    }
    shown = flat;
    paintSel();
  }

  // O texto digitado vira tarefa para hoje, sem sair da tela em que se está.
  function quickTask(q) {
    if (q.length < 2) return [];
    return [{
      group: 'shell.palette.group.actions', icon: 'plus', label: t('shell.cmd.task-create', { q }),
      run: () => addTask(q),
    }];
  }
  async function addTask(title) {
    const d = new Date();
    const pad = (n) => String(n).padStart(2, '0');
    const today = d.getFullYear() + '-' + pad(d.getMonth() + 1) + '-' + pad(d.getDate());
    await invoke('task_add', { task: { title, planned_on: today } });
    if (current !== 'today') toast(t('shell.task.created'), 'ok', 4000, { action: { label: t('shell.task.open'), run: () => show('today') } });
  }

  function paintSel() {
    list.querySelectorAll('.pal-item').forEach((r, i) => r.setAttribute('aria-selected', String(i === sel)));
    const row = $('pal-opt-' + sel);
    if (row) { input.setAttribute('aria-activedescendant', row.id); row.scrollIntoView({ block: 'nearest' }); }
  }

  function runAt(i) {
    const it = shown[i];
    if (!it) return;
    pal.close();
    Promise.resolve().then(() => it.run()).catch((e) => toast(t('common.error', { e }), 'err'));
  }

  // Reuniões pelo título, resumo e transcript — a mesma busca da Biblioteca.
  function searchMeetings() {
    clearTimeout(searchTimer);
    const q = input.value.trim();
    if (q.length < 2) { meetings = []; return; }
    const seq = ++searchSeq;
    searchTimer = setTimeout(async () => {
      let rows = [];
      try { rows = await invoke('list_meetings', { query: q }); } catch (_) { return; }
      if (seq !== searchSeq || !pal.open) return;
      meetings = rows.slice(0, 6).map((r) => ({
        group: 'shell.palette.group.meetings', icon: 'doc', label: r.title, meta: r.started_at,
        run: () => invoke('open_library_window', { meeting: r.id }),
      }));
      render();
    }, 140);
  }

  function openPalette() {
    if (pal.open) return;
    items = buildItems();
    meetings = [];
    sel = 0;
    input.value = '';
    render();
    pal.showModal();
    input.focus();
  }
  input.addEventListener('input', () => { sel = 0; meetings = []; render(); searchMeetings(); });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); sel = (sel + 1) % Math.max(1, shown.length); paintSel(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); sel = (sel - 1 + shown.length) % Math.max(1, shown.length); paintSel(); }
    else if (e.key === 'Enter') { e.preventDefault(); runAt(sel); }
    else if (e.key === 'Home' && e.ctrlKey) { sel = 0; paintSel(); }
  });
  pal.addEventListener('click', (e) => { if (e.target === pal) pal.close(); });

  // --------------------------------------------------------------- idioma
  document.addEventListener('isper-i18n', () => { paintRail(); paintStatus(); });

  // ------------------------------------------------------------------ início
  // Pedidos do app: bandeja, notificações, botões das outras telas.
  listen('isper-nav', (e) => {
    const r = e.payload || {};
    const params = r.section ? { section: r.section } : null;
    show(r.view, params, { fromRust: true });
  });

  paintRail();
  const first = boot.route && VIEWS[boot.route.view] ? boot.route.view : 'home';
  show(first, boot.route && boot.route.section ? { section: boot.route.section } : null, { fromRust: true, focus: false });
  refreshStatus();
  // A janela nasce escondida: aparece quando a primeira tela pintou. A rota
  // pode ter mudado enquanto ela carregava (outro pedido do app).
  frames.get(first).ready.then(async () => {
    try {
      const r = await invoke('shell_ready');
      if (r && r.view && r.view !== current) show(r.view, null, { fromRust: true });
    } catch (_) {}
    // A Biblioteca é a tela mais visitada: pronta antes do primeiro clique.
    setTimeout(() => { if (current !== 'onboarding') prefetch('library'); }, 1200);
  });
})();
