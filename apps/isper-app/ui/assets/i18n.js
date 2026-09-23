// ISPer — tradução da interface (sem build step; expõe window.I18N e t()).
//
// Os dicionários vivem em ui/locales/<idioma>.json e entram no binário pelo
// Rust (i18n.rs), que também os usa para bandeja, notificações e erros —
// uma só fonte de verdade. A janela recebe o dicionário no nascimento
// (window.__ISPER_UI.strings, via script de inicialização), então t() é
// síncrono desde o primeiro script da página e nada pisca em português para
// quem usa inglês. Janelas que nascem sem ele (o indicador, criado pela
// config do Tauri) pedem por invoke('ui_prefs').
//
// No HTML:
//   <h2 data-i18n="settings.system.title">Sistema</h2>
//   <input data-i18n-attr="placeholder:library.search.placeholder">
// O texto em pt-BR do HTML fica como está: é a própria referência e o que
// aparece se a chave faltar (o teste de paridade impede que falte).
//
// No JS:  t('library.count', { n: 3 })  →  "3 reuniões" / "3 meetings"
// Plural: a chave aponta para um objeto { "one": "...", "other": "..." } e
// a variável `n` escolhe (Intl.PluralRules do idioma).
(function () {
  'use strict';

  var prefs = window.__ISPER_UI || {};
  var lang = prefs.lang || 'pt-BR';
  var strings = prefs.strings || null;
  var rules = null;

  function pluralRules() {
    if (!rules) {
      try { rules = new Intl.PluralRules(lang); } catch (_) { rules = new Intl.PluralRules('pt-BR'); }
    }
    return rules;
  }

  function lookup(key) {
    if (!strings) return undefined;
    return Object.prototype.hasOwnProperty.call(strings, key) ? strings[key] : undefined;
  }

  function fill(text, vars) {
    if (!vars) return text;
    return text.replace(/\{(\w+)\}/g, function (m, name) {
      if (!Object.prototype.hasOwnProperty.call(vars, name)) return m;
      var v = vars[name];
      return typeof v === 'number' ? v.toLocaleString(lang) : String(v);
    });
  }

  // t(chave, variáveis, reserva): a reserva é o texto original em pt-BR, para
  // código que rode antes do dicionário (nunca deveria acontecer).
  function t(key, vars, fallback) {
    var v = lookup(key);
    if (v && typeof v === 'object') {
      var n = vars && typeof vars.n === 'number' ? vars.n : 0;
      v = v[pluralRules().select(n)] || v.other;
    }
    if (typeof v !== 'string') {
      if (window.__isperErrors && strings) window.__isperErrors.push('i18n: chave ausente ' + key);
      v = fallback != null ? fallback : key;
    }
    return fill(v, vars);
  }

  // Texto com marcação mínima (<code>, <b>, <kbd>), sem innerHTML: só esses
  // três elementos viram elementos; o resto é texto.
  function setRich(node, text) {
    node.replaceChildren();
    var last = 0;
    for (const m of text.matchAll(/<(code|b|kbd)>([\s\S]*?)<\/\1>/g)) {
      if (m.index > last) node.appendChild(document.createTextNode(text.slice(last, m.index)));
      var e = document.createElement(m[1]);
      e.textContent = m[2];
      node.appendChild(e);
      last = m.index + m[0].length;
    }
    if (last < text.length) node.appendChild(document.createTextNode(text.slice(last)));
  }

  function translate(root) {
    if (!strings) return;
    var scope = root || document;
    scope.querySelectorAll('[data-i18n]').forEach(function (n) {
      var v = lookup(n.dataset.i18n);
      if (typeof v === 'string') n.textContent = v;
    });
    scope.querySelectorAll('[data-i18n-html]').forEach(function (n) {
      var v = lookup(n.dataset.i18nHtml);
      if (typeof v === 'string') setRich(n, v);
    });
    scope.querySelectorAll('[data-i18n-attr]').forEach(function (n) {
      n.dataset.i18nAttr.split(',').forEach(function (pair) {
        var i = pair.indexOf(':');
        if (i < 0) return;
        var v = lookup(pair.slice(i + 1).trim());
        if (typeof v === 'string') n.setAttribute(pair.slice(0, i).trim(), v);
      });
    });
    if (!root) {
      document.documentElement.lang = lang;
      var tk = document.documentElement.dataset.i18nTitle;
      if (tk && typeof lookup(tk) === 'string') document.title = lookup(tk);
    }
  }

  var readyResolve;
  var ready = new Promise(function (r) { readyResolve = r; });

  function start() {
    translate();
    readyResolve();
  }
  function whenDom(fn) {
    if (document.readyState === 'loading') addEventListener('DOMContentLoaded', fn);
    else fn();
  }

  if (strings) {
    whenDom(start);
  } else {
    // Sem dicionário no nascimento (indicador): pede ao app.
    var T = window.__TAURI__;
    var ask = T && T.core ? T.core.invoke('ui_prefs') : Promise.reject(new Error('sem Tauri'));
    ask.then(function (p) {
      lang = p.lang || lang;
      strings = p.strings || null;
      rules = null;
      whenDom(start);
    }).catch(function () { readyResolve(); });
  }

  // Idioma trocado nas Configurações: o app manda o dicionário novo a todas
  // as janelas. O HTML estático é retraduzido aqui; o que a página monta por
  // script ela refaz no evento `isper-i18n`.
  function listenChanges() {
    var T = window.__TAURI__;
    if (!T || !T.event) return;
    T.event.listen('isper-ui', function (e) {
      var p = (e && e.payload) || {};
      if (!p.strings || !p.lang) return;
      var changed = p.lang !== lang;
      lang = p.lang;
      strings = p.strings;
      rules = null;
      translate();
      if (changed) document.dispatchEvent(new Event('isper-i18n'));
    });
  }
  if (window.__TAURI__) listenChanges();
  else addEventListener('DOMContentLoaded', listenChanges);

  window.I18N = {
    t: t,
    translate: translate,
    setRich: setRich,
    ready: ready,
    get lang() { return lang; },
    // Formatação no idioma da interface.
    number: function (n, opts) { return Number(n).toLocaleString(lang, opts); },
  };
  window.t = t;
})();
