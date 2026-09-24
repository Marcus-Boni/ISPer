#!/usr/bin/env python3
"""Banco de testes da janela do Copilot (apps/isper-app/ui/copilot.html).

Serve a pasta `ui/` num servidor local e injeta um `window.__TAURI__` de
mentira em `copilot.html`, com uma reunião roteirizada. Assim dá para mexer
no HUD — layout, estados vazios, erro, teclado, sidecar de 380 px — sem
recompilar o app inteiro (o build com CUDA leva minutos).

    python tools/e2e/copilot-harness.py          # http://127.0.0.1:3112/copilot.html
                                                # http://127.0.0.1:3112/library.html
                                                # http://127.0.0.1:3112/home.html (só a barra)

No console da página:

    __sim.scenario('meeting')   # reunião cheia (padrão)
    __sim.scenario('idle')      # nenhuma reunião em andamento
    __sim.scenario('no-key')    # sem provider de IA configurado
    __sim.scenario('error')     # última análise falhou
    __sim.play()                # despeja o roteiro inteiro de uma vez
    __sim.step()                # avança uma fala
    __sim.analyzing(true)       # liga/desliga o estado "analisando"
    __sim.failStream(true)      # a próxima resposta cai no meio do streaming
    __sim.lang('en')            # troca o idioma na hora, como o app faz
    __sim.endMeeting()          # a reunião acaba e é salva, com as notas
    __sim.newMeeting()          # outra reunião começa com a janela aberta

A página recebe o mesmo dicionário que o app injeta no nascimento da janela
(`ui/locales/`); `?lang=en` na URL abre direto em inglês.

O que o mock NÃO cobre: a captura de áudio, o Whisper e as chamadas de IA
de verdade. Ele exercita a camada de tela — que é onde mora a maior parte
dos detalhes de usabilidade.
"""

import http.server
import json
import os
import socketserver
import sys
import urllib.parse

ROOT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
    "apps", "isper-app", "ui",
)
PORT = int(os.environ.get("ISPER_HARNESS_PORT", "3112"))

MOCK = r"""
<script>
/* --- Tauri de mentira: o suficiente para a copilot.html rodar no navegador. */
(function () {
  const listeners = {};
  const emit = (name, payload) => (listeners[name] || []).forEach((cb) => cb({ payload }));

  const SPEECH = [
    ['Eu', 0, 6, 'Bom dia pessoal, obrigado por virem. Queria fechar hoje o escopo da fase dois.'],
    ['Participante 1', 7, 15, 'Bom dia! Pelo nosso lado o orçamento aprovado foi de cinquenta mil reais para o trimestre.'],
    ['Eu', 16, 23, 'Perfeito. E a data de entrega que vocês conseguem assumir com esse orçamento?'],
    ['Participante 2', 24, 34, 'Conseguimos entregar dia trinta, mas discordo de incluir o módulo de relatórios agora. Me preocupa o prazo.'],
    ['Eu', 35, 44, 'Entendo a preocupação. Vamos deixar relatórios para a fase três então.'],
    ['Participante 1', 45, 52, 'Então fica combinado: entrega dia trinta, sem o módulo de relatórios.'],
    ['Eu', 53, 61, 'Fechado. Eu envio a proposta revisada com esse escopo ainda esta semana.'],
    ['Participante 2', 62, 72, 'Só uma dúvida sobre o SLA de fim de semana — isso não ficou claro no contrato anterior.'],
    ['Participante 1', 73, 84, 'Boa pergunta. O Carlos vai avaliar a cobertura de fim de semana e traz na próxima.'],
    ['Eu', 85, 96, 'Combinado. Vou preparar também um comparativo de custo entre as duas opções de infraestrutura.'],
  ];

  const CARDS = [
    { id: 'dec-entrega-dia-30-sem-relatorios', kind: 'decision',
      title: 'Entrega dia 30, sem relatórios',
      description: 'Escopo da fase dois fechado: entrega no dia 30, módulo de relatórios adiado para a fase três.',
      urgency: 'high', at_secs: 45, status: 'proposed' },
    { id: 'act-enviar-proposta-revisada', kind: 'action',
      title: 'Enviar proposta revisada',
      description: 'Proposta com o escopo acordado (sem relatórios).',
      owner: 'Eu', due_date: 'Esta semana', urgency: 'medium', at_secs: 53, status: 'proposed' },
    { id: 'act-avaliar-sla-de-fim-de-semana', kind: 'action',
      title: 'Avaliar SLA de fim de semana',
      description: 'Verificar a cobertura de fim de semana e trazer na próxima reunião.',
      owner: 'Carlos', due_date: 'Sem prazo definido', urgency: 'high', at_secs: 73, status: 'proposed' },
    { id: 'rsk-sla-de-fim-de-semana-em-aberto', kind: 'risk',
      title: 'SLA de fim de semana em aberto',
      description: 'Ponto levantado pelo Participante 2 e ainda sem resposta objetiva.',
      urgency: 'medium', at_secs: 62, status: 'proposed' },
    { id: 'qst-confirmar-multa-por-atraso', kind: 'question',
      title: 'Confirmar multa por atraso',
      description: 'Vale perguntar se a data 30 tem penalidade contratual antes de encerrar.',
      urgency: 'low', at_secs: 84, status: 'proposed' },
    { id: 'dec-orcamento-de-r-50-mil', kind: 'decision',
      title: 'Orçamento de R$ 50 mil',
      description: 'Valor aprovado pelo cliente para o trimestre.',
      urgency: 'medium', at_secs: 7, status: 'confirmed' },
    { id: 'rsk-ruido-na-sala', kind: 'risk',
      title: 'Ruído na sala',
      description: 'Card de baixa relevância, descartado pelo usuário.',
      urgency: 'low', at_secs: 3, status: 'discarded' },
  ];

  const S = {
    scenario: 'meeting',
    cursor: 0,
    cards: JSON.parse(JSON.stringify(CARDS)),
    scratchpad: '- prazo da entrega\n- quem fica com o SLA',
    generation: 1,
    savedMeeting: null,   // id da reunião em que as notas já foram salvas
    analyzing: false,
    error: null,
    failStream: false,
    memories: [
      { id: 'mem-7', meeting_id: 7, title: 'Negociacao com o mesmo cliente',
        started_at: '10/09/2026 15:30', at_secs: 412, score: 0.71,
        snippet: 'Ficou combinado o preco de quarenta mil para o mesmo escopo, com entrega em 45 dias.' },
      { id: 'mem-3', meeting_id: 3, title: 'Retrospectiva da fase um',
        started_at: '02/09/2026 09:00', at_secs: 95, score: 0.62,
        snippet: 'O modulo de relatorios foi o que mais atrasou a fase um.' },
    ],
  };

  function dto() {
    const meeting = S.scenario !== 'idle';
    const spoken = SPEECH.slice(0, S.cursor);
    let me = 0, others = 0;
    spoken.forEach(([who, a, b]) => { const d = b - a; if (who === 'Eu') me += d; else others += d; });
    return {
      meeting_active: meeting,
      configured: S.scenario !== 'no-key',
      running: S.analyzing,
      active_topic: S.cursor > 3 ? 'Escopo e data de entrega da fase dois' : '',
      cards: S.scenario === 'idle' ? [] : S.cards,
      dynamics_note: null,
      memories: S.cursor > 3 ? S.memories : [],
      me_talk_secs: me,
      others_talk_secs: others,
      monologue: false,
      elapsed_secs: spoken.length ? SPEECH[S.cursor - 1][2] : 0,
      scratchpad: S.scratchpad,
      last_updated: S.cursor > 3 ? '10:42:07' : null,
      last_trigger: S.cursor > 5 ? 'decision' : null,
      error: S.scenario === 'error' ? 'status 429: limite de requisições do provider' : S.error,
      generation: S.generation,
      notes_saved: S.savedMeeting !== null,
    };
  }

  const seg = (row, provisional) => ({
    speaker: row[0], start_secs: row[1], end_secs: row[2], text: row[3], provisional: !!provisional,
  });

  const push = () => emit('isper-copilot', dto());

  /* O Rust manda pedaços por `isper-copilot-token` e só resolve o comando no
     fim; aqui é o mesmo contrato, em câmera lenta para dar para ver. */
  function streamOut(id, full, stepMs) {
    const parts = full.match(/.{1,12}/gs) || [];
    return new Promise((resolve) => {
      let i = 0;
      const tick = () => {
        if (i >= parts.length) return resolve(full);
        emit('isper-copilot-token', { id, chunk: parts[i++] });
        setTimeout(tick, stepMs == null ? 45 : stepMs);
      };
      setTimeout(tick, 120);
    });
  }

  /* Falha no meio do fluxo: a tela precisa manter o que já chegou. */
  function streamThenFail(id, parcial) {
    return new Promise((_, reject) => {
      emit('isper-copilot-token', { id, chunk: parcial });
      setTimeout(() => reject('a conexão caiu no meio da resposta'), 250);
    });
  }

  window.__TAURI__ = {
    event: {
      listen(name, cb) {
        (listeners[name] = listeners[name] || []).push(cb);
        return Promise.resolve(() => {});
      },
    },
    core: {
      invoke(cmd, args) {
        args = args || {};
        switch (cmd) {
          case 'copilot_get_state':
            return Promise.resolve(dto());
          case 'live_transcript':
            return Promise.resolve(SPEECH.slice(0, S.cursor).map((r) => seg(r, false)));
          case 'copilot_card_action': {
            const c = S.cards.find((x) => x.id === args.cardId);
            if (!c) return Promise.reject('card não encontrado');
            if (args.action === 'confirm') c.status = 'confirmed';
            else if (args.action === 'discard') c.status = 'discarded';
            else if (args.action === 'reset') c.status = 'proposed';
            setTimeout(push, 0);
            return Promise.resolve();
          }
          case 'copilot_dismiss_memory':
            S.memories = S.memories.filter((m) => m.id !== args.memoryId);
            setTimeout(push, 0);
            return Promise.resolve();
          case 'open_library_window':
            return Promise.resolve();
          case 'copilot_save_scratchpad':
            S.scratchpad = args.text;
            // Depois de salva a reunião, a edição vai para ela.
            return Promise.resolve(S.savedMeeting !== null);
          case 'copilot_analyze_now':
            if (S.scenario === 'idle') return Promise.reject('nenhuma reunião em andamento');
            window.__sim.analyzing(true);
            setTimeout(() => window.__sim.analyzing(false), 1800);
            return Promise.resolve();
          case 'copilot_query':
            if (S.failStream) return streamThenFail(args.streamId, 'O orçamento citado foi ');
            return streamOut(args.streamId,
              '- O orçamento citado foi de R$ 50.000 para o trimestre.\n\n- A data acordada é dia 30, sem o módulo de relatórios.');
          case 'copilot_enrich_notes':
            if (S.failStream) return streamThenFail(args.streamId, '- **Prazo da entrega:** dia 30');
            return streamOut(args.streamId,
              '- **Prazo da entrega:** dia 30, escopo sem o módulo de relatórios (acordado aos 00:45).\n- **SLA de fim de semana:** Carlos ficou de avaliar — sem prazo definido (00:73).');
          case 'copilot_set_always_on_top':
            return Promise.resolve();
          case 'mark_moment_cmd':
            return Promise.resolve(S.cursor ? SPEECH[S.cursor - 1][1] : 0);
          case 'open_settings_window':
            return Promise.resolve();
          default:
            return Promise.reject('comando não simulado: ' + cmd);
        }
      },
    },
  };

  window.__sim = {
    step() {
      if (S.cursor >= SPEECH.length) return false;
      const row = SPEECH[S.cursor++];
      emit('isper-live', seg(row, false));
      const d = dto();
      emit('isper-copilot-metrics', {
        me_talk_secs: d.me_talk_secs, others_talk_secs: d.others_talk_secs,
        monologue: d.monologue, elapsed_secs: d.elapsed_secs,
      });
      push();
      return true;
    },
    partial(text) { emit('isper-live', seg(['Eu', 97, 99, text], true)); },
    play() { while (this.step()) {} return S.cursor; },
    analyzing(on) { S.analyzing = !!on; push(); },
    scenario(name) {
      S.scenario = name;
      if (name === 'idle') { S.cursor = 0; }
      push();
      return name;
    },
    reset() { S.cursor = 0; S.cards = JSON.parse(JSON.stringify(CARDS)); S.failStream = false; push(); },
    failStream(on) { S.failStream = !!on; return S.failStream; },
    // Fim da reunião: salva no banco, e o bloco fica ligado a ela.
    endMeeting() { S.scenario = 'idle'; S.savedMeeting = 42; push(); return S.savedMeeting; },
    // Outra reunião começa com a janela aberta: estado novo, bloco vazio.
    newMeeting() {
      S.generation += 1; S.savedMeeting = null; S.scratchpad = '';
      S.scenario = 'meeting'; S.cursor = 0; S.cards = JSON.parse(JSON.stringify(CARDS));
      push();
      return S.generation;
    },
    // Troca o idioma como o app faz (evento isper-ui com o dicionário novo).
    lang(name) {
      const strings = (window.__ISPER_LOCALES || {})[name];
      if (!strings) return 'idioma desconhecido: ' + name;
      emit('isper-ui', { theme: 'dark', lang: name, strings });
      return name;
    },
    state: () => dto(),
  };
})();
</script>
"""


LIB_MOCK = r"""
<script>
/* --- Tauri de mentira para a Biblioteca: uma reuniao com decisoes. */
(function () {
  const listeners = {};
  const SEGS = [
    ['Eu', 0, 6, 'Bom dia pessoal, queria fechar hoje o escopo da fase dois.'],
    ['Participante 1', 7, 15, 'O orcamento aprovado foi de cinquenta mil reais para o trimestre.'],
    ['Participante 2', 24, 34, 'Conseguimos entregar dia trinta, mas discordo de incluir relatorios agora.'],
    ['Participante 1', 45, 52, 'Entao fica combinado: entrega dia trinta, sem o modulo de relatorios.'],
    ['Eu', 53, 61, 'Fechado. Eu envio a proposta revisada ainda esta semana.'],
    ['Participante 1', 73, 84, 'O Carlos vai avaliar a cobertura de fim de semana e traz na proxima.'],
  ];
  const DECISIONS = [
    { kind: 'decision', title: 'Entrega dia 30, sem relatorios',
      description: 'Escopo da fase dois fechado; relatorios ficam para a fase tres.',
      owner: null, due_date: null, urgency: 'high', at_secs: 45 },
    { kind: 'action', title: 'Enviar proposta revisada',
      description: 'Proposta com o escopo acordado.',
      owner: 'Eu', due_date: 'Esta semana', urgency: 'medium', at_secs: 53 },
    { kind: 'action', title: 'Avaliar SLA de fim de semana',
      description: 'Verificar a cobertura e trazer na proxima reuniao.',
      owner: 'Carlos', due_date: 'Sem prazo definido', urgency: 'high', at_secs: 73 },
    { kind: 'risk', title: 'SLA de fim de semana em aberto',
      description: 'Levantado pelo Participante 2 e ainda sem resposta objetiva.',
      owner: null, due_date: null, urgency: 'medium', at_secs: 24 },
  ];
  const ROW = {
    id: 1, title: 'Alinhamento da fase dois', started_at: '21/09/2026 10:00',
    duration_secs: 96, segments: SEGS.length, participants: 2,
    has_summary: true, md_path: 'C:/x/reuniao.md', moments: 1,
    decisions: DECISIONS.length,
  };
  const SEM_DECISOES = Object.assign({}, ROW, {
    id: 2, title: 'Conversa sem Copilot', decisions: 0, moments: 0, has_summary: false,
  });

  window.__TAURI__ = {
    event: {
      listen(name, cb) {
        (listeners[name] = listeners[name] || []).push(cb);
        return Promise.resolve(() => {});
      },
    },
    core: {
      invoke(cmd, args) {
        args = args || {};
        switch (cmd) {
          case 'list_meetings': return Promise.resolve([ROW, SEM_DECISOES]);
          case 'list_dictations': return Promise.resolve([]);
          case 'take_pending_meeting': return Promise.resolve(null);
          case 'embeddings_status': return Promise.resolve({ configured: false, key_present: false });
          case 'semantic_search': return Promise.resolve([]);
          // Como o app: grava a escolha e avisa as janelas (evento isper-ui).
          case 'set_ui_theme':
            (listeners['isper-ui'] || []).forEach((cb) => cb({ payload: { theme: args.theme } }));
            return Promise.resolve(args.theme);
          case 'get_meeting': {
            const meeting = args.id === 2 ? SEM_DECISOES : ROW;
            return Promise.resolve({
              meeting,
              summary: args.id === 2 ? null : 'Escopo da fase dois fechado com entrega no dia 30.',
              segments: SEGS.map((r) => ({ speaker: r[0], start_secs: r[1], end_secs: r[2], text: r[3] })),
              moments: args.id === 2 ? [] : [45],
              decisions: args.id === 2 ? [] : DECISIONS,
              notes: args.id === 2 ? null : '- prazo: dia 30, sem relatórios\n- Carlos avalia o SLA de fim de semana\n\n<b>isto é texto, não HTML</b>',
            });
          }
          default: return Promise.resolve(null);
        }
      },
    },
  };
  window.__lib = {
    decisions: () => DECISIONS,
    // O tema mudou em outra janela (ou nas Configurações).
    theme(name) { (listeners['isper-ui'] || []).forEach((cb) => cb({ payload: { theme: name } })); return name; },
  };
})();
</script>
"""


def ui_prefs_script(path):
    """window.__ISPER_UI com o dicionário do idioma pedido (?lang=, padrão
    pt-BR) e window.__ISPER_LOCALES com os dois, para __sim.lang()."""
    query = urllib.parse.parse_qs(urllib.parse.urlparse(path).query)
    lang = (query.get("lang") or ["pt-BR"])[0]
    locales = {}
    for name in ("pt-BR", "en"):
        with open(os.path.join(ROOT, "locales", name + ".json"), encoding="utf-8") as fh:
            locales[name] = json.load(fh)
    if lang not in locales:
        lang = "pt-BR"
    prefs = {"theme": "dark", "lang": lang, "strings": locales[lang]}
    return ("<script>window.__ISPER_UI = " + json.dumps(prefs, ensure_ascii=False)
            + "; window.__ISPER_LOCALES = " + json.dumps(locales, ensure_ascii=False)
            + ";</script>\n")


class Handler(http.server.SimpleHTTPRequestHandler):
    # HTTP/1.1 + servidor com threads: o navegador abre várias conexões em
    # paralelo (css, js, fontes) e um servidor de uma thread só trava.
    protocol_version = "HTTP/1.1"

    def __init__(self, *a, **kw):
        super().__init__(*a, directory=ROOT, **kw)

    def do_GET(self):  # noqa: N802
        rota = self.path.split("?")[0]
        alvo = {"/": "copilot.html", "/copilot.html": "copilot.html",
                "/library.html": "library.html", "/home.html": "home.html"}.get(rota)
        if alvo:
            path = os.path.join(ROOT, alvo)
            with open(path, encoding="utf-8") as fh:
                html = fh.read()
            # O Início usa o mock da Biblioteca: o que ele pede e o mock não
            # conhece volta vazio, o bastante para exercitar a barra do topo.
            mock = LIB_MOCK if alvo in ("library.html", "home.html") else MOCK
            # O app injeta o dicionário no nascimento da janela (ui.rs,
            # boot_script); aqui, o mesmo: ?lang=en abre em inglês.
            html = html.replace("<head>", "<head>\n" + ui_prefs_script(self.path) + mock, 1)
            body = html.encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)
            return
        super().do_GET()

    def log_message(self, fmt, *args):
        sys.stderr.write("%s\n" % (fmt % args))


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


if __name__ == "__main__":
    with Server(("127.0.0.1", PORT), Handler) as httpd:
        print("copilot harness: http://127.0.0.1:%d/copilot.html" % PORT, flush=True)
        httpd.serve_forever()
