// Verificacao de acessibilidade rodada DENTRO da pagina (via cdp.mjs, pelo
// a11y.ps1). Sem dependencia externa: cobre o que mais pesa no ISPer.
//  - nome acessivel de todo controle visivel (aria-labelledby, aria-label,
//    <label>, texto, title ou placeholder);
//  - contraste WCAG 2.x AA de todo texto visivel (4.5:1; 3:1 para texto
//    grande), contra o fundo efetivo (camadas semitransparentes somadas);
//  - lang no documento.
// Devolve JSON: { lang, names: [...], contrast: [...] }.
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
    if (cs.display === 'none' || cs.visibility === 'hidden' || Number(cs.opacity) === 0) return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const describe = (el) => {
    let s = el.tagName.toLowerCase();
    if (el.id) s += '#' + el.id;
    else if (typeof el.className === 'string' && el.className.trim()) s += '.' + el.className.trim().split(/\s+/).slice(0, 2).join('.');
    const txt = (el.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 30);
    return txt ? s + ' "' + txt + '"' : s;
  };
  const textOf = (ids) => ids.split(/\s+/).map((id) => {
    const n = document.getElementById(id);
    return n ? n.textContent.trim() : '';
  }).join(' ').trim();
  const accName = (el) => {
    const lb = el.getAttribute('aria-labelledby');
    if (lb && textOf(lb)) return textOf(lb);
    const al = el.getAttribute('aria-label');
    if (al && al.trim()) return al.trim();
    if (el.labels && el.labels.length) {
      const t = [...el.labels].map((l) => l.textContent.trim()).join(' ').trim();
      if (t) return t;
    }
    if (!['INPUT', 'SELECT', 'TEXTAREA'].includes(el.tagName)) {
      const t = (el.innerText || el.textContent || '').trim();
      if (t) return t;
    }
    const title = el.getAttribute('title');
    if (title && title.trim()) return title.trim();
    const ph = el.getAttribute('placeholder');
    if (ph && ph.trim()) return ph.trim();
    return '';
  };
  const INTERACTIVE = 'button, a[href], input:not([type=hidden]), select, textarea, [role=button], [role=switch], [role=tab], [role=radio], [role=checkbox], [tabindex]:not([tabindex="-1"])';
  const names = [];
  for (const el of document.querySelectorAll(INTERACTIVE)) {
    // Rotulo visual com o input real escondido (switch, radio estilizado): vale o rotulo.
    const shown = visible(el) || (el.tagName === 'INPUT' && visible(el.closest('label')));
    if (!shown) continue;
    if (!accName(el)) names.push(describe(el));
  }

  const parse = (c) => {
    let m = c.match(/^rgba?\(([^)]+)\)$/);
    if (m) {
      const p = m[1].split(/[\s,/]+/).filter(Boolean).map(Number);
      return [p[0], p[1], p[2], p.length > 3 ? p[3] : 1];
    }
    m = c.match(/^color\(srgb ([^)]+)\)$/);
    if (m) {
      const p = m[1].split(/[\s/]+/).filter(Boolean).map(Number);
      return [p[0] * 255, p[1] * 255, p[2] * 255, p.length > 3 ? p[3] : 1];
    }
    return null;
  };
  const blend = (top, bottom) => {
    const a = top[3];
    return [top[0] * a + bottom[0] * (1 - a), top[1] * a + bottom[1] * (1 - a), top[2] * a + bottom[2] * (1 - a), 1];
  };
  const background = (el) => {
    const layers = [];
    for (let n = el; n; n = n.parentElement) {
      const c = parse(getComputedStyle(n).backgroundColor);
      if (c && c[3] > 0) {
        layers.push(c);
        if (c[3] >= 1) break;
      }
    }
    let bg = [255, 255, 255, 1];
    for (let i = layers.length - 1; i >= 0; i--) bg = blend(layers[i], bg);
    return bg;
  };
  const lum = (c) => {
    const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); };
    return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
  };
  const ratio = (a, b) => {
    const la = lum(a), lb = lum(b);
    return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
  };
  const contrast = [];
  const seen = new Set();
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let t = walker.nextNode(); t; t = walker.nextNode()) {
    if (!t.textContent.trim()) continue;
    const el = t.parentElement;
    if (!el || seen.has(el) || !visible(el)) continue;
    seen.add(el);
    // Texto desativado e placeholder nao entram no criterio 1.4.3 (WCAG).
    if (el.closest('button:disabled, [aria-disabled="true"], input:disabled, select:disabled, option')) continue;
    const cs = getComputedStyle(el);
    const fg0 = parse(cs.color);
    if (!fg0) continue;
    const bg = background(el);
    const fg = fg0[3] < 1 ? blend(fg0, bg) : fg0;
    const size = parseFloat(cs.fontSize);
    const bold = Number(cs.fontWeight) >= 700;
    const need = (size >= 24 || (bold && size >= 18.66)) ? 3 : 4.5;
    const r = ratio(fg, bg);
    if (r + 0.005 < need) contrast.push(describe(el) + ' ' + r.toFixed(2) + ':1 (precisa ' + need + ')');
  }
  return JSON.stringify({ lang: document.documentElement.lang, names, contrast });
})()
