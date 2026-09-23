// Preparacao da volta de Tab (a11y.ps1): marca cada controle que o teclado
// PRECISA alcancar (visivel, ativo, tabIndex >= 0; num grupo de radios, so o
// marcado - ou o primeiro -, que e onde o Tab para) e instala
// window.__a11yStep, que o cdp-keys.mjs chama depois de cada tecla para
// registrar onde o foco esta e se ele aparece (:focus-visible com anel).
// Depois, window.__a11yReport() devolve o que faltou.
// ASCII puro: o PowerShell 5.1 le o arquivo como ANSI.
(() => {
  const visible = (el) => {
    if (!el || !el.isConnected) return false;
    if (el.closest('[hidden], [aria-hidden="true"], [inert]')) return false;
    // Conteudo de <details> fechado conta como escondido (o <summary> nao).
    for (let d = el.closest('details'); d; d = d.parentElement && d.parentElement.closest('details')) {
      const sum = d.querySelector(':scope > summary');
      if (!d.open && !(sum && sum.contains(el))) return false;
    }
    const cs = getComputedStyle(el);
    if (cs.display === 'none' || cs.visibility === 'hidden') return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const describe = (el) => {
    let s = el.tagName.toLowerCase();
    if (el.id) s += '#' + el.id;
    else if (typeof el.className === 'string' && el.className.trim()) s += '.' + el.className.trim().split(/\s+/).slice(0, 2).join('.');
    const txt = (el.textContent || el.value || '').trim().replace(/\s+/g, ' ').slice(0, 24);
    return txt ? s + ' "' + txt + '"' : s;
  };
  const SEL = 'button, a[href], input:not([type=hidden]), select, textarea, [role=button], [role=tab], [tabindex]';
  const candidates = [...document.querySelectorAll(SEL)].filter((el) => {
    if (el.disabled || el.tabIndex < 0) return false;
    // Input real escondido atras de um rotulo estilizado: vale o rotulo visivel.
    return visible(el) || (el.tagName === 'INPUT' && visible(el.closest('label')));
  });
  const must = candidates.filter((el) => {
    if (el.type !== 'radio') return true;
    const group = [...document.querySelectorAll('input[type=radio]')].filter((r) => r.name === el.name && !r.disabled);
    return el === (group.find((r) => r.checked) || group[0]);
  });
  document.querySelectorAll('[data-a11y-idx]').forEach((el) => el.removeAttribute('data-a11y-idx'));
  must.forEach((el, i) => { el.dataset.a11yIdx = String(i); });
  // O anel pode estar no proprio controle ou no rotulo que o desenha.
  const ringOf = (el) => {
    const hosts = [el];
    if (el.tagName === 'INPUT') {
      const sw = el.closest('.switch');
      if (sw) hosts.push(sw.querySelector('.track'));
      const lab = el.closest('label');
      if (lab) hosts.push(lab);
    }
    return hosts.filter(Boolean).some((h) => {
      const cs = getComputedStyle(h);
      return (cs.outlineStyle !== 'none' && parseFloat(cs.outlineWidth) > 0) || (cs.boxShadow && cs.boxShadow !== 'none');
    });
  };
  const state = { seen: [], noRing: [], foreign: [] };
  window.__a11yStep = () => {
    const a = document.activeElement;
    if (!a || a === document.body || a === document.documentElement) { state.seen.push(-1); return; }
    const idx = a.dataset.a11yIdx;
    if (idx === undefined) state.foreign.push(describe(a));
    else state.seen.push(Number(idx));
    if (!a.matches(':focus-visible') || !ringOf(a)) state.noRing.push(describe(a));
  };
  window.__a11yReport = () => {
    const reached = new Set(state.seen.filter((i) => i >= 0));
    const missed = must.filter((_, i) => !reached.has(i)).map(describe);
    const first = state.seen.find((i) => i >= 0);
    const wrapped = first !== undefined && state.seen.filter((i) => i === first).length >= 2;
    return JSON.stringify({ total: must.length, reached: reached.size, missed, noRing: [...new Set(state.noRing)], foreign: [...new Set(state.foreign)], wrapped });
  };
  if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
  return JSON.stringify({ total: must.length });
})()
