// ISPer — primeiro script do <head>: liga a tela à janela e aplica o tema
// antes da primeira pintura. As preferências chegam em window.__ISPER_UI
// (script de inicialização que o app injeta ao criar a janela) e as
// mudanças, pelo evento `isper-ui`. "system" deixa a escolha para o
// prefers-color-scheme do Windows.
(function () {
  'use strict';
  var root = document.documentElement;

  // Tela dentro da janela principal (um iframe do app.html): o Tauri só
  // injeta o __TAURI__ e as preferências no frame principal. A tela usa os
  // da página-mãe — mesma origem —, então os comandos e eventos são os
  // mesmos de quando cada tela era uma janela própria. Os ouvintes de evento
  // registrados aqui saem junto com a tela (pagehide), para não sobrarem
  // presos à mãe quando ela descarta um iframe.
  var host = null;
  try {
    if (window.parent !== window && window.parent.__TAURI__) host = window.parent;
  } catch (_) {}
  if (host) {
    var T = host.__TAURI__;
    var unlisten = [];
    var event = Object.assign({}, T.event, {
      listen: function (name, handler, options) {
        var p = T.event.listen(name, handler, options);
        p.then(function (u) { unlisten.push(u); }, function () {});
        return p;
      },
    });
    window.__TAURI__ = Object.assign({}, T, { event: event });
    window.__ISPER_UI = host.__ISPER_UI;
    root.dataset.embedded = '';
    addEventListener('pagehide', function () {
      unlisten.splice(0).forEach(function (u) { try { u(); } catch (_) {} });
    });
    // Atalhos da janela (Ctrl+K, Ctrl+1…) com o foco dentro da tela. Fica na
    // window: os atalhos da própria tela (no document) vêm antes e podem
    // cancelar o padrão.
    addEventListener('keydown', function (e) {
      var shell = host.ISPER_SHELL;
      if (!e.defaultPrevented && shell && shell.key(e)) e.preventDefault();
    });
    addEventListener('mouseup', function (e) {
      var shell = host.ISPER_SHELL;
      if ((e.button === 3 || e.button === 4) && shell) { e.preventDefault(); shell.mouse(e.button); }
    });
    // Para a tela avisar a janela (ex.: alterações não salvas).
    window.ISPER_SHELL = {
      setDirty: function (view, on) { if (host.ISPER_SHELL) host.ISPER_SHELL.setDirty(view, on); },
    };
  }

  var prefs = window.__ISPER_UI || {};

  function applyTheme(theme) {
    root.dataset.theme = theme === 'light' || theme === 'dark' ? theme : 'system';
  }
  applyTheme(prefs.theme);

  function listen() {
    var T = window.__TAURI__;
    if (!T || !T.event) return;
    T.event.listen('isper-ui', function (e) {
      var p = (e && e.payload) || {};
      if (p.theme) applyTheme(p.theme);
    });
  }
  if (window.__TAURI__) listen();
  else addEventListener('DOMContentLoaded', listen);
})();
