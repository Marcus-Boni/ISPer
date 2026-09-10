// ISPer — helpers de interface compartilhados (sem build step; expõe window.UI).
// Microinterações com estado: toast, count-up, entrada escalonada, botões
// ocupados/sucesso, confirmação inline em dois passos, abas com indicador
// deslizante, skeleton. Tudo constrói DOM via createElement (nunca HTML solto).
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
  function toast(msg, kind = 'info', ms = 2800) {
    if (!host) {
      host = el('div', 'toast-host');
      host.setAttribute('role', 'status');
      host.setAttribute('aria-live', 'polite');
      document.body.appendChild(host);
    }
    const t = el('div', 'toast toast-' + kind);
    t.appendChild(el('span', 'toast-ic', kind === 'ok' ? '✓' : kind === 'err' ? '!' : 'i'));
    t.appendChild(el('span', 'toast-msg', msg));
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

  // -------------------------------------------------------------- count-up
  function countUp(node, to, opts = {}) {
    const { dur = 720, format = (n) => n.toLocaleString('pt-BR') } = opts;
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

  // ------------------------------------ confirmação inline (sem modal nativo)
  function confirmInline(btn, onYes, opts = {}) {
    const { label = 'Confirmar?', ms = 3600 } = opts;
    const disarm = () => {
      if (btn.dataset.armed !== '1') return;
      btn.dataset.armed = '0';
      btn.classList.remove('is-armed');
      btn.textContent = btn.dataset.label;
      delete btn.dataset.label;
    };
    if (btn.dataset.armed === '1') {
      clearTimeout(Number(btn.dataset.timer));
      disarm();
      onYes();
      return;
    }
    btn.dataset.armed = '1';
    btn.dataset.label = btn.textContent;
    btn.textContent = label;
    btn.classList.add('is-armed');
    btn.style.setProperty('--arm-ms', ms + 'ms');
    btn.dataset.timer = String(setTimeout(disarm, ms));
  }

  // ------------------------------------------- abas com indicador deslizante
  function tabs(container, onChange) {
    const ind = container.querySelector('.tab-ind') || container.appendChild(el('span', 'tab-ind'));
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
    return (secs / 3600).toFixed(1).replace('.', ',') + ' h';
  };
  const plural = (n, one, many) => n + ' ' + (n === 1 ? one : many);

  window.UI = { el, toast, countUp, stagger, busy, flash, confirmInline, tabs, swap, skeleton, fmtClock, fmtDur, plural, reduce };
})();
