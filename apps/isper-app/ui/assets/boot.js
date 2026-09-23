// ISPer — primeiro script do <head>: aplica o tema antes da primeira pintura.
// As preferências chegam em window.__ISPER_UI (script de inicialização que o
// app injeta ao criar a janela) e as mudanças, pelo evento `isper-ui`.
// "system" deixa a escolha para o prefers-color-scheme do Windows.
(function () {
  'use strict';
  var root = document.documentElement;
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
