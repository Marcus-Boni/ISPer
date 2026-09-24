// ISPer — helpers de interface compartilhados (sem build step; expõe window.UI).
// Microinterações com estado: toast, count-up, entrada escalonada, botões
// ocupados/sucesso, desfazer (exclusão adiada), abas com indicador deslizante,
// skeleton. Tudo constrói DOM via createElement (nunca HTML solto).
(function () {
  'use strict';

  const reduce = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  };

  // Erros de runtime ficam acessíveis para diagnóstico (DevTools remoto / suporte).
  window.__isperErrors = window.__isperErrors || [];
  addEventListener('error', (e) => window.__isperErrors.push(String(e.message || e)));
  addEventListener('unhandledrejection', (e) => window.__isperErrors.push('rejection: ' + String(e.reason)));
  // Violação de CSP não dispara 'error': sem isto, um recurso bloqueado falharia
  // em silêncio. O smoke test (tools/e2e) lê esta lista em cada janela.
  document.addEventListener('securitypolicyviolation', (e) =>
    window.__isperErrors.push('csp: ' + e.violatedDirective + ' bloqueou ' + (e.blockedURI || 'inline') + ' em ' + e.sourceFile + ':' + e.lineNumber));

  // Entrada escalonada declarada no HTML: `data-i="2"` vira `--i: 2` via CSSOM
  // (a CSP não permite style="--i:2" no atributo).
  document.querySelectorAll('[data-i]').forEach((n) => n.style.setProperty('--i', n.dataset.i));

  // ----------------------------------------------------------------- toast
  let host = null;
  // opts.action = { label, run }: um botão no toast (ex.: Desfazer). Com ação,
  // uma barra mostra o tempo que falta e o toast não some ao passar o mouse.
  function toast(msg, kind = 'info', ms = 2800, opts = {}) {
    if (!host) {
      host = el('div', 'toast-host');
      host.setAttribute('role', 'status');
      host.setAttribute('aria-live', 'polite');
      document.body.appendChild(host);
    }
    const t = el('div', 'toast toast-' + kind);
    if (kind === 'err') t.setAttribute('role', 'alert');
    const ic = el('span', 'toast-ic', kind === 'ok' ? '✓' : kind === 'err' ? '!' : 'i');
    ic.setAttribute('aria-hidden', 'true');
    t.appendChild(ic);
    t.appendChild(el('span', 'toast-msg', msg));
    if (opts.action) {
      const b = el('button', 'toast-act', opts.action.label);
      b.type = 'button';
      b.addEventListener('click', (ev) => { ev.stopPropagation(); close(); opts.action.run(); });
      t.appendChild(b);
      const bar = el('i', 'toast-timer');
      bar.style.setProperty('--toast-ms', ms + 'ms');
      t.appendChild(bar);
      t.classList.add('has-action');
    }
    host.appendChild(t);
    while (host.children.length > 3) host.firstElementChild.remove();
    requestAnimationFrame(() => requestAnimationFrame(() => t.classList.add('in')));
    let done = false;
    const close = () => {
      if (done) return;
      done = true;
      t.classList.remove('in');
      t.classList.add('out');
      setTimeout(() => t.remove(), 260);
    };
    const h = setTimeout(close, ms);
    t.addEventListener('click', () => { clearTimeout(h); close(); });
    return close;
  }

  // ------------------------------------------------------------- desfazer
  // Exclusão já agendada no app (resposta { token, undo_ms }): mostra o
  // "Desfazer" pelo tempo que o app espera antes de apagar de fato. Ctrl+Z
  // também desfaz enquanto o toast está na tela. `onUndo(ok)` recebe se deu
  // tempo (false = o app já tinha apagado).
  function undoable(msg, sched, onUndo) {
    const undoLabel = (window.I18N ? window.I18N.t('common.undo', null, 'Desfazer') : 'Desfazer');
    let done = false;
    let closeToast = () => {};
    const run = async () => {
      if (done) return;
      done = true;
      removeEventListener('keydown', onKey, true);
      let ok = false;
      try { ok = await window.__TAURI__.core.invoke('undo_delete', { token: sched.token }); } catch (_) { ok = false; }
      onUndo(ok);
    };
    const onKey = (ev) => {
      if ((ev.ctrlKey || ev.metaKey) && !ev.shiftKey && ev.key.toLowerCase() === 'z') {
        const tag = (ev.target && ev.target.tagName) || '';
        if (tag === 'INPUT' || tag === 'TEXTAREA') return; // o Ctrl+Z do campo é do campo
        ev.preventDefault();
        closeToast();
        run();
      }
    };
    addEventListener('keydown', onKey, true);
    const ms = Math.max(1500, (sched.undo_ms || 7000) - 400);
    closeToast = toast(msg, 'ok', ms, { action: { label: undoLabel, run } });
    setTimeout(() => { if (!done) { done = true; removeEventListener('keydown', onKey, true); } }, ms);
  }

  // Idioma da interface para números e datas (pt-BR antes do i18n.js carregar).
  const uiLang = () => (window.I18N && window.I18N.lang) || 'pt-BR';

  // -------------------------------------------------------------- count-up
  function countUp(node, to, opts = {}) {
    const { dur = 720, format = (n) => n.toLocaleString(uiLang()) } = opts;
    const from = Number(node.dataset.value || 0);
    node.dataset.value = String(to);
    if (reduce() || from === to || !isFinite(to)) { node.textContent = format(to); return; }
    const t0 = performance.now();
    const tick = (t) => {
      const p = Math.min(1, (t - t0) / dur);
      const e = 1 - Math.pow(1 - p, 3);
      node.textContent = format(Math.round(from + (to - from) * e));
      if (p < 1) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }

  // ----------------------------------------------------- entrada escalonada
  function stagger(nodes, max = 12) {
    let i = 0;
    for (const n of nodes) {
      n.style.setProperty('--i', String(Math.min(i++, max)));
      n.classList.add('reveal');
    }
  }

  // ----------------------------------------------------- botões com estado
  function busy(btn, on, label) {
    if (on) {
      if (!('label' in btn.dataset)) btn.dataset.label = btn.textContent;
      btn.classList.add('is-loading');
      btn.disabled = true;
      btn.setAttribute('aria-busy', 'true');
      if (label) btn.textContent = label;
    } else {
      btn.classList.remove('is-loading');
      btn.disabled = false;
      btn.removeAttribute('aria-busy');
      if ('label' in btn.dataset) { btn.textContent = btn.dataset.label; delete btn.dataset.label; }
    }
  }
  function flash(btn, label, kind = 'ok', ms = 1500) {
    const prev = btn.textContent;
    const wasDisabled = btn.disabled;
    btn.classList.add('is-' + kind);
    btn.textContent = label;
    btn.disabled = true;
    setTimeout(() => {
      btn.classList.remove('is-' + kind);
      btn.textContent = prev;
      btn.disabled = wasDisabled;
    }, ms);
  }

  // ------------------------------------------- abas com indicador deslizante
  // Abas acessíveis (padrão WAI-ARIA): tablist/tab/aria-selected, só a aba
  // ativa entra no Tab e as setas (← → Home End) trocam de aba.
  function tabs(container, onChange) {
    const ind = container.querySelector('.tab-ind') || container.appendChild(el('span', 'tab-ind'));
    ind.setAttribute('aria-hidden', 'true');
    container.setAttribute('role', 'tablist');
    const all = () => [...container.querySelectorAll('.tab')];
    const sync = () => all().forEach((x) => {
      const on = x.classList.contains('on');
      x.setAttribute('role', 'tab');
      x.setAttribute('aria-selected', on ? 'true' : 'false');
      x.tabIndex = on ? 0 : -1;
    });
    sync();
    container.addEventListener('keydown', (ev) => {
      const list = all();
      const i = list.indexOf(document.activeElement);
      if (i < 0) return;
      const to = ev.key === 'ArrowRight' ? (i + 1) % list.length
        : ev.key === 'ArrowLeft' ? (i - 1 + list.length) % list.length
          : ev.key === 'Home' ? 0 : ev.key === 'End' ? list.length - 1 : -1;
      if (to < 0) return;
      ev.preventDefault();
      list[to].focus();
      list[to].click();
    });
    const place = () => {
      const on = container.querySelector('.tab.on');
      if (!on) return;
      const c = container.getBoundingClientRect();
      const r = on.getBoundingClientRect();
      ind.style.width = r.width + 'px';
      ind.style.transform = 'translateX(' + (r.left - c.left - container.clientLeft) + 'px)';
    };
    container.querySelectorAll('.tab').forEach((t) => t.addEventListener('click', () => {
      if (t.classList.contains('on')) return;
      container.querySelectorAll('.tab').forEach((x) => x.classList.toggle('on', x === t));
      sync();
      place();
      if (onChange) onChange(t.dataset.tab, t);
    }));
    addEventListener('resize', place);
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(place);
    requestAnimationFrame(place);
    return {
      place,
      select(name) {
        const t = container.querySelector('.tab[data-tab="' + name + '"]');
        if (t && !t.classList.contains('on')) t.click();
      },
    };
  }

  // ------------------------------------------- troca de conteúdo com fade
  function swap(node, render) {
    node.classList.remove('swap');
    render();
    void node.offsetWidth;
    node.classList.add('swap');
  }

  // --------------------------------------------------------------- skeleton
  function skeleton(container, rows = 4) {
    container.replaceChildren();
    for (let i = 0; i < rows; i++) {
      const r = el('div', 'skeleton-row');
      r.style.opacity = String(1 - i * 0.18);
      r.appendChild(el('div', 'skeleton sk-title'));
      r.appendChild(el('div', 'skeleton sk-line'));
      container.appendChild(r);
    }
  }

  // ------------------------------------------------------------ formatação
  const fmtClock = (s) => {
    s = Math.max(0, Math.floor(s));
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
    return (h ? h + ':' : '') + String(m).padStart(2, '0') + ':' + String(r).padStart(2, '0');
  };
  const fmtDur = (secs) => {
    if (secs < 60) return Math.round(secs) + ' s';
    if (secs < 3600) return Math.round(secs / 60) + ' min';
    return (secs / 3600).toLocaleString(uiLang(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }) + ' h';
  };
  const plural = (n, one, many) => n + ' ' + (n === 1 ? one : many);
  // Rótulo de atalho vindo do app ("Ctrl+Alt+Espaço") no idioma da interface.
  // Rótulo de falante é dado (fica em pt-BR no banco); só a exibição traduz.
  const tt = (k, v, f) => (window.I18N ? window.I18N.t(k, v, f) : f);
  const speaker = (label) => {
    if (label === 'Eu') return tt('speaker.me', null, 'Eu');
    if (label === 'Participantes') return tt('speaker.others', null, 'Participantes');
    const m = String(label || '').match(/^Participante (\d+)$/);
    return m ? tt('speaker.participant', { n: Number(m[1]) }, label) : (label || tt('speaker.others', null, 'Participantes'));
  };
  // Descrição do modelo Whisper no idioma da interface (o catálogo do
  // isper-models, usado também pela CLI, fica em pt-BR).
  const modelNote = (m) => ({
    'ggml-large-v3-turbo-q5_0.bin': tt('model.note.turbo', null, m.note),
    'ggml-small.bin': tt('model.note.small', null, m.note),
    'ggml-medium-q5_0.bin': tt('model.note.medium', null, m.note),
    'ggml-large-v3-q5_0.bin': tt('model.note.large', null, m.note),
  })[m.file] || m.note;
  const keyLabel = (label) => String(label || '').replace(/Espaço/g, window.I18N ? window.I18N.t('keys.space', null, 'Espaço') : 'Espaço');

  // Tema claro/escuro num clique, fora das Configurações. O botão troca o
  // tema que está NA TELA pelo oposto — com "seguir o Windows", o que o
  // Windows está mostrando agora. Voltar a seguir o Windows é nas
  // Configurações. A troca vale para todas as janelas (set_ui_theme avisa
  // cada uma), e o botão acompanha o que vier de fora: outra janela, as
  // Configurações ou o próprio Windows mudando de tema.
  function themeToggle(btn) {
    if (!btn) return;
    const root = document.documentElement;
    const dark = matchMedia('(prefers-color-scheme: dark)');
    const t = (k) => tt(k, null, k);
    const shown = () => {
      const th = root.dataset.theme;
      if (th === 'light' || th === 'dark') return th;
      return dark.matches ? 'dark' : 'light';
    };
    const paint = () => {
      const next = shown() === 'dark' ? 'light' : 'dark';
      btn.dataset.next = next;
      const label = next === 'light' ? t('common.theme.to-light') : t('common.theme.to-dark');
      btn.title = label;
      btn.setAttribute('aria-label', label);
    };
    btn.addEventListener('click', async () => {
      const invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
      if (!invoke) return;
      try { await invoke('set_ui_theme', { theme: btn.dataset.next }); }
      catch (e) { toast(String(e), 'err'); }
    });
    new MutationObserver(paint).observe(root, { attributes: true, attributeFilter: ['data-theme'] });
    dark.addEventListener('change', paint);
    document.addEventListener('isper-i18n', paint);
    paint();
  }

  window.UI = { el, toast, themeToggle, undoable, keyLabel, speaker, modelNote, countUp, stagger, busy, flash, tabs, swap, skeleton, fmtClock, fmtDur, plural, reduce, uiLang };
})();
