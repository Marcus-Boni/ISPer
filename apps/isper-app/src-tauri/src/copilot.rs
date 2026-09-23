//! Copilot de Decisões e Notetaker em tempo real no ISPer App (Fase 8).
//!
//! Gerencia o estado cognitivo da reunião:
//! - Cards de Decisão, Ação, Risco e Perguntas Recomendadas;
//! - Q&A in-meeting ("Pergunte à Reunião");
//! - Bloco de notas aumentado estilo Granola;
//! - Métricas de dinâmica (tempo de fala de "Eu" vs "Participantes").
//!
//! ## Por que existem dois eventos
//!
//! `isper-copilot` carrega o estado inteiro (cards, notas, tópico) e só sai
//! quando esse estado muda de verdade — rodada da IA, ação do usuário. Já
//! `isper-copilot-metrics` é um par de números e sai a cada fala, com
//! garganta de [`METRICS_EVERY`]. Mandar o estado inteiro a cada bloco
//! transcrito era clonar todos os cards e o bloco de notas várias vezes por
//! minuto — e, pior, devolvia o scratchpad para a tela no meio da digitação.
//!
//! ## Por que a análise não espera o pulso
//!
//! O pulso periódico ([`COPILOT_AUTO_INTERVAL_SECS`]) é a rede de segurança.
//! O caminho rápido é [`isper_llm::detect_trigger`]: assim que alguém diz
//! "então fica combinado", a rodada é antecipada, respeitando um intervalo
//! mínimo ([`MIN_GAP_BETWEEN_ROUNDS_SECS`]) para não torrar API à toa.
//!
//! ## Por que só com a janela aberta
//!
//! Até a 0.20 o loop analisava toda reunião gravada com provedor configurado,
//! com a janela aberta ou não — perto de 80 chamadas por hora que ninguém
//! estava vendo. Agora rodada, gatilho e memória só acontecem com a janela
//! visível (e não minimizada); o loop confere a cada [`WATCH_TICK`] e, quando
//! ela aparece, lê a conversa na hora. O que é local continua sempre: a fala
//! ao vivo e a dinâmica de fala.
//!
//! ## Notas
//!
//! O bloco de notas vai com a reunião: entra na ata e no banco quando ela é
//! salva ([`CopilotWrapUp`]) e, depois disso, cada edição regrava os dois
//! ([`copilot_save_scratchpad`]).

use crate::prelude::*;
use std::sync::mpsc;

/// Quanto de conversa recente vai no prompt de análise.
const COPILOT_WINDOW_SECS: f32 = 20.0 * 60.0;
/// Quanto de conversa recente vai no Q&A e no enriquecimento de notas.
/// Sem isto, uma reunião de 2 h mandaria o transcript inteiro a cada pergunta.
const QA_WINDOW_SECS: f32 = 30.0 * 60.0;
/// Abaixo disto de texto novo, o pulso periódico não gasta uma chamada.
const MIN_NEW_CHARS_COPILOT: usize = 120;
/// Na PRIMEIRA leitura o gate é menor: no começo da reunião qualquer coisa
/// concreta já vale um card, e ficar calado dá a impressão de não funcionar.
const MIN_NEW_CHARS_FIRST: usize = 60;
/// Rede de segurança: mesmo sem gatilho, o Copilot revisita a conversa.
const COPILOT_AUTO_INTERVAL_SECS: u64 = 45;
/// Quando acontece a primeira leitura da reunião.
///
/// Esperar o pulso inteiro fazia o HUD passar quase um minuto sem nada na
/// tela — e um assistente que fica mudo no começo parece quebrado, mesmo
/// estando certo em não ter o que dizer ainda.
const FIRST_ROUND_SECS: u64 = 20;
/// Piso entre duas rodadas de IA, para o gatilho não virar metralhadora.
const MIN_GAP_BETWEEN_ROUNDS_SECS: f32 = 15.0;
/// De quanto em quanto tempo o loop confere se a janela está aberta.
///
/// É também o atraso máximo entre abrir a janela e a primeira leitura: curto
/// para parecer imediato, longo o bastante para não ocupar a thread principal
/// (cada consulta de visibilidade passa por ela).
const WATCH_TICK: Duration = Duration::from_secs(2);
/// Garganta do evento de métricas (dinâmica de fala).
const METRICS_EVERY: Duration = Duration::from_millis(1500);
/// Fala contínua de "Eu" acima disto acende o aviso de monólogo.
const MONOLOGUE_SECS: f32 = 180.0;
/// Quantas reuniões passadas o Copilot lembra de uma vez.
const RECALL_LIMIT: usize = 3;
/// Abaixo desta semelhança (cosseno), a reunião passada é ruído.
///
/// É o botão de sinal/ruído da memória: subir deixa o Copilot mais calado e
/// mais certeiro; descer traz mais contexto e mais engano. 0,55 foi escolhido
/// para errar para o lado de calado — um card de memória errado no meio de
/// uma reunião custa mais atenção do que vale.
const RECALL_MIN_SCORE: f32 = 0.55;
/// De quanto em quanto tempo os pedaços do streaming vão para a tela.
///
/// Um evento IPC por token seriam dezenas por segundo para nada: a 20 Hz
/// o texto já sai fluido e o custo some.
const STREAM_FLUSH_EVERY: Duration = Duration::from_millis(50);

pub(crate) enum CopilotCmd {
    Now,
    Stop,
}

/// O que o loop faz numa volta.
#[derive(Debug, PartialEq, Eq)]
enum Turn {
    /// Nada nesta volta.
    Idle,
    /// Pedido explícito — o botão Analisar ou um gatilho: roda mesmo sem
    /// conversa nova.
    Forced,
    /// A janela acabou de aparecer: roda se houver conversa nova.
    Opened,
    /// Hora do pulso, com a janela na tela.
    Pulse,
    /// Hora do pulso, mas ninguém está olhando: só reagenda.
    SkipPulse,
}

/// A regra do custo numa função só: sem a janela na tela, o pulso não chama a
/// IA. Os pedidos explícitos já chegam filtrados — o botão só existe com a
/// janela aberta, e o gatilho confere `watching` antes de pedir.
fn turn(asked: bool, opened: bool, watching: bool, pulse_due: bool) -> Turn {
    if asked {
        Turn::Forced
    } else if opened {
        Turn::Opened
    } else if pulse_due && watching {
        Turn::Pulse
    } else if pulse_due {
        Turn::SkipPulse
    } else {
        Turn::Idle
    }
}

pub(crate) struct CopilotAppState {
    pub(crate) active_topic: String,
    pub(crate) cards: Vec<isper_llm::CopilotCard>,
    pub(crate) dynamics_note: Option<String>,
    pub(crate) running: bool,
    pub(crate) error: Option<String>,
    pub(crate) last_updated: Option<String>,
    pub(crate) scratchpad: String,
    pub(crate) tx: Option<mpsc::Sender<CopilotCmd>>,
    pub(crate) seen_chars: usize,
    /// Tempo de fala acumulado da reunião inteira.
    ///
    /// Não dá para somar a partir de `state.live`: aquela lista é podada em
    /// [`LIVE_KEEP`] segmentos, então numa reunião longa o medidor passaria a
    /// mostrar só o fim da conversa.
    pub(crate) me_secs: f32,
    pub(crate) others_secs: f32,
    /// Fala contínua de "Eu" desde a última vez que outra pessoa falou.
    pub(crate) me_streak_secs: f32,
    /// O que acordou a última rodada ("acordo detectado"), para a UI explicar.
    pub(crate) last_trigger: Option<String>,
    /// Reuniões passadas que falam do assunto do momento.
    pub(crate) memories: Vec<MemoryHit>,
    /// Memórias que o usuário dispensou — não voltam nesta reunião.
    pub(crate) dismissed_memories: Vec<String>,
    /// Último tópico já pesquisado: sem isto, cada rodada gastaria um
    /// embedding para reencontrar exatamente as mesmas reuniões.
    pub(crate) recalled_topic: Option<String>,
    /// Qual reunião este estado representa.
    ///
    /// Encerrar a reunião não mata na hora a thread de análise: ela pode
    /// estar parada dentro de uma chamada HTTP e só voltar segundos depois.
    /// Se outra reunião já tiver começado nesse meio-tempo, o resultado
    /// atrasado cairia no estado NOVO — cards da reunião passada aparecendo
    /// na atual, e a limpeza da thread velha apagando o `tx` da nova (o que
    /// desliga os gatilhos em silêncio). Cada thread guarda a geração com
    /// que nasceu e só encosta no estado se ela ainda for a corrente.
    pub(crate) generation: u64,
    /// A janela está aberta, visível e não minimizada — lido pelo loop a cada
    /// [`WATCH_TICK`]. Guardado aqui porque o gatilho roda no caminho quente
    /// da transcrição e não pode perguntar à janela a cada fala.
    pub(crate) watching: bool,
    /// Em que reunião do banco estas notas já foram salvas. A partir daí, cada
    /// edição vai direto para ela.
    pub(crate) saved_meeting: Option<i64>,
    last_round: Option<Instant>,
    last_metrics_emit: Option<Instant>,
}

impl Default for CopilotAppState {
    fn default() -> Self {
        Self {
            // Vazio até a primeira leitura: a tela mostra "aguardando" no idioma dela.
            active_topic: String::new(),
            cards: Vec::new(),
            dynamics_note: None,
            running: false,
            error: None,
            last_updated: None,
            scratchpad: String::new(),
            tx: None,
            seen_chars: 0,
            me_secs: 0.0,
            others_secs: 0.0,
            me_streak_secs: 0.0,
            last_trigger: None,
            memories: Vec::new(),
            dismissed_memories: Vec::new(),
            recalled_topic: None,
            generation: 0,
            watching: false,
            saved_meeting: None,
            last_round: None,
            last_metrics_emit: None,
        }
    }
}

impl CopilotAppState {
    /// Contabiliza uma fala fechada no medidor de dinâmica.
    pub(crate) fn note_speech(&mut self, speaker: &str, secs: f32) {
        let secs = secs.max(0.0);
        if speaker == "Eu" {
            self.me_secs += secs;
            self.me_streak_secs += secs;
        } else {
            self.others_secs += secs;
            self.me_streak_secs = 0.0;
        }
    }

    /// As notas mais recentes da reunião que `wrap` fechou: o bloco de agora,
    /// se o estado ainda é dela (o usuário pode ter continuado escrevendo
    /// enquanto a transcrição terminava), ou o que havia ao encerrar, se outra
    /// reunião já começou e zerou o bloco.
    pub(crate) fn notes_for(&self, wrap: &CopilotWrapUp) -> String {
        if self.generation == wrap.generation {
            self.scratchpad.clone()
        } else {
            wrap.notes.clone()
        }
    }

    /// Está em monólogo? (fala contínua de "Eu" passando do limite)
    pub(crate) fn monologue(&self) -> bool {
        self.me_streak_secs >= MONOLOGUE_SECS
    }

    /// Mescla o resultado de uma rodada preservando o que o usuário decidiu.
    ///
    /// Card já confirmado ou descartado não é tocado: a IA repete o mesmo
    /// ponto rodada após rodada, e sobrescrever apagaria a validação manual.
    pub(crate) fn merge_cards(&mut self, incoming: Vec<isper_llm::CopilotCard>) -> usize {
        let mut novos = 0;
        for new_card in incoming {
            match self
                .cards
                .iter_mut()
                .find(|c| isper_llm::is_same_card(c, &new_card))
            {
                Some(existing) => {
                    if existing.status == isper_llm::CardStatus::Proposed {
                        // `at_secs` fica no primeiro avistamento: é a hora em
                        // que o assunto surgiu, não a da última releitura.
                        existing.title = new_card.title;
                        existing.description = new_card.description;
                        existing.owner = new_card.owner;
                        existing.due_date = new_card.due_date;
                        existing.urgency = new_card.urgency;
                    }
                }
                None => {
                    self.cards.push(new_card);
                    novos += 1;
                }
            }
        }
        novos
    }

    pub(crate) fn apply_card_action(
        &mut self,
        card_id: &str,
        action: &str,
    ) -> Result<(), &'static str> {
        if action == "delete" {
            let before = self.cards.len();
            self.cards.retain(|c| c.id != card_id);
            return if self.cards.len() < before {
                Ok(())
            } else {
                Err("card não encontrado")
            };
        }

        let card = self
            .cards
            .iter_mut()
            .find(|c| c.id == card_id)
            .ok_or("card não encontrado")?;

        match action {
            "confirm" => {
                card.status = isper_llm::CardStatus::Confirmed;
                Ok(())
            }
            "discard" => {
                card.status = isper_llm::CardStatus::Discarded;
                Ok(())
            }
            // Desfazer: devolve o card à fila de propostos.
            "reset" => {
                card.status = isper_llm::CardStatus::Proposed;
                Ok(())
            }
            _ => Err("ação inválida"),
        }
    }
}

/// Há provider de IA com chave guardada? (a UI explica o que falta)
pub(crate) fn copilot_configured() -> bool {
    let llm = isper_llm::load_settings();
    !llm.provider.trim().is_empty()
        && isper_llm::get_api_key(&llm.provider)
            .ok()
            .flatten()
            .is_some()
}

/// Uma reunião passada que trata do assunto em discussão agora.
#[derive(Clone, serde::Serialize, PartialEq)]
pub(crate) struct MemoryHit {
    /// `mem-<id da reunião>` — uma memória por reunião passada.
    pub(crate) id: String,
    pub(crate) meeting_id: i64,
    pub(crate) title: String,
    pub(crate) started_at: String,
    /// Instante do trecho dentro daquela reunião, quando há.
    pub(crate) at_secs: Option<f32>,
    pub(crate) snippet: String,
    pub(crate) score: f32,
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct CopilotDto {
    pub(crate) meeting_active: bool,
    pub(crate) configured: bool,
    pub(crate) running: bool,
    pub(crate) active_topic: String,
    pub(crate) cards: Vec<isper_llm::CopilotCard>,
    pub(crate) dynamics_note: Option<String>,
    pub(crate) memories: Vec<MemoryHit>,
    pub(crate) me_talk_secs: f32,
    pub(crate) others_talk_secs: f32,
    pub(crate) monologue: bool,
    /// Cronômetro da reunião; a tela anda sozinha entre um evento e outro.
    pub(crate) elapsed_secs: f32,
    pub(crate) scratchpad: String,
    pub(crate) last_updated: Option<String>,
    pub(crate) last_trigger: Option<String>,
    pub(crate) error: Option<String>,
    /// Qual reunião a tela está vendo. Quando muda, o bloco de notas da tela
    /// é da reunião anterior e precisa ser trocado pelo desta.
    pub(crate) generation: u64,
    /// As notas já foram salvas com a reunião (e cada edição regrava).
    pub(crate) notes_saved: bool,
}

/// Só a dinâmica de fala — o que pode sair a cada bloco transcrito.
#[derive(Clone, serde::Serialize)]
pub(crate) struct CopilotMetricsDto {
    pub(crate) me_talk_secs: f32,
    pub(crate) others_talk_secs: f32,
    pub(crate) monologue: bool,
    pub(crate) elapsed_secs: f32,
}

pub(crate) fn copilot_dto(app: &AppHandle) -> CopilotDto {
    let state = app.state::<AppState>();
    let started = *state.meeting_started.lock_or_recover();
    let meeting_active = started.is_some();
    let elapsed_secs = started.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0);
    let configured = copilot_configured();

    let cop = state.copilot.lock_or_recover();
    CopilotDto {
        meeting_active,
        configured,
        running: cop.running,
        active_topic: cop.active_topic.clone(),
        cards: cop.cards.clone(),
        dynamics_note: cop.dynamics_note.clone(),
        memories: cop.memories.clone(),
        me_talk_secs: cop.me_secs,
        others_talk_secs: cop.others_secs,
        monologue: cop.monologue(),
        elapsed_secs,
        scratchpad: cop.scratchpad.clone(),
        last_updated: cop.last_updated.clone(),
        last_trigger: cop.last_trigger.clone(),
        error: cop.error.clone(),
        generation: cop.generation,
        notes_saved: cop.saved_meeting.is_some(),
    }
}

pub(crate) fn emit_copilot(app: &AppHandle) {
    let _ = app.emit("isper-copilot", copilot_dto(app));
}

/// Uma fala fechada chegou: atualiza a dinâmica e, se o texto denuncia um
/// acordo/tarefa/objeção, antecipa a rodada de análise.
///
/// Chamado no caminho quente da transcrição — por isso só toca em contadores
/// e num `contains` local, nada de rede nem de clonar o estado inteiro.
pub(crate) fn on_live_segment(app: &AppHandle, speaker: &str, secs: f32, text: &str) {
    let state = app.state::<AppState>();
    let elapsed_secs = state
        .meeting_started
        .lock_or_recover()
        .map(|t| t.elapsed().as_secs_f32())
        .unwrap_or(0.0);

    let (metrics, nudge) = {
        let mut cop = state.copilot.lock_or_recover();
        cop.note_speech(speaker, secs);

        // Gatilho local: de graça, sem rede, sobre o texto que acabou de sair.
        let nudge = match isper_llm::detect_trigger(text) {
            Some(kind) => {
                let ready = cop
                    .last_round
                    .map(|t| t.elapsed().as_secs_f32() >= MIN_GAP_BETWEEN_ROUNDS_SECS)
                    .unwrap_or(true);
                // Janela fechada: ninguém está olhando, então nada de gastar
                // uma chamada — a rodada sai quando ela abrir.
                if ready && !cop.running && cop.watching {
                    // Nome estável; a tela traduz (copilot.trigger.<nome>).
                    cop.last_trigger = Some(kind.as_str().to_string());
                    cop.tx.clone()
                } else {
                    None
                }
            }
            None => None,
        };

        let due = cop
            .last_metrics_emit
            .map(|t| t.elapsed() >= METRICS_EVERY)
            .unwrap_or(true);
        let metrics = if due {
            cop.last_metrics_emit = Some(Instant::now());
            Some(CopilotMetricsDto {
                me_talk_secs: cop.me_secs,
                others_talk_secs: cop.others_secs,
                monologue: cop.monologue(),
                elapsed_secs,
            })
        } else {
            None
        };
        (metrics, nudge)
    };

    if let Some(m) = metrics {
        let _ = app.emit("isper-copilot-metrics", m);
    }
    if let Some(tx) = nudge {
        let _ = tx.send(CopilotCmd::Now);
    }
}

/// Início da reunião: reinicia o estado e sobe o loop inteligente do Copilot.
pub(crate) fn reset_copilot(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut cop = state.copilot.lock_or_recover();
        if let Some(tx) = cop.tx.take() {
            let _ = tx.send(CopilotCmd::Stop);
        }
        // A geração avança ANTES de zerar: qualquer thread da reunião anterior
        // que ainda esteja no ar passa a ver um número diferente do seu e se
        // recolhe sem tocar neste estado.
        let next = cop.generation.wrapping_add(1);
        *cop = CopilotAppState::default();
        cop.generation = next;
    }
    if copilot_configured() {
        ensure_copilot_loop(app);
    }
    emit_copilot(app);
}

/// Fim da reunião: encerra o loop de fundo do Copilot.
///
/// Os cards ficam: [`finish_meeting`] ainda vai ler os confirmados para a ata,
/// e a janela do HUD continua aberta mostrando o que foi decidido.
pub(crate) fn stop_copilot_loop(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut cop = state.copilot.lock_or_recover();
    if let Some(tx) = cop.tx.take() {
        let _ = tx.send(CopilotCmd::Stop);
    }
    cop.running = false;
}

/// O que o Copilot deixa para a reunião que acabou: os cards validados (seção
/// da ata e linhas da aba "Decisões") e as notas.
///
/// Precisa ser lido no momento em que a reunião encerra, e não lá dentro do
/// `finish_meeting` (que roda numa thread e ainda espera o worker do Whisper):
/// começar outra reunião nesse intervalo zeraria o estado e a ata da reunião
/// que acabou sairia sem nada. A geração diz, mais tarde, se o estado ainda é
/// desta reunião — e portanto se as notas podem ter sido editadas depois.
pub(crate) struct CopilotWrapUp {
    generation: u64,
    pub(crate) decisions: Vec<isper_llm::CopilotCard>,
    notes: String,
}

pub(crate) fn copilot_wrap_up(app: &AppHandle) -> CopilotWrapUp {
    let state = app.state::<AppState>();
    let cop = state.copilot.lock_or_recover();
    CopilotWrapUp {
        generation: cop.generation,
        decisions: cop
            .cards
            .iter()
            .filter(|c| c.status == isper_llm::CardStatus::Confirmed)
            .cloned()
            .collect(),
        notes: cop.scratchpad.clone(),
    }
}

/// As notas mais recentes da reunião que `wrap` fechou ([`CopilotAppState::notes_for`]).
pub(crate) fn wrap_up_notes(app: &AppHandle, wrap: &CopilotWrapUp) -> String {
    app.state::<AppState>()
        .copilot
        .lock_or_recover()
        .notes_for(wrap)
}

/// A reunião acabou de entrar no banco: grava as notas nela e, se o estado
/// ainda é desta reunião, liga o bloco a ela — dali em diante, cada edição
/// vai direto para o banco e para a ata.
///
/// Leitura e gravação acontecem com o estado travado, como em
/// [`copilot_save_scratchpad`]: uma edição que chegue no meio não pode ser
/// sobrescrita por um texto mais velho.
pub(crate) fn attach_saved_meeting(
    app: &AppHandle,
    wrap: &CopilotWrapUp,
    store: &isper_core::store::MeetingStore,
    meeting_id: i64,
) {
    {
        let state = app.state::<AppState>();
        let mut cop = state.copilot.lock_or_recover();
        let current = cop.generation == wrap.generation;
        let notes = cop.notes_for(wrap);
        if let Err(e) = store.set_notes(meeting_id, &notes) {
            tracing::warn!("não consegui guardar as notas do Copilot: {e}");
            return;
        }
        if current {
            cop.saved_meeting = Some(meeting_id);
        }
    }
    emit_copilot(app);
}

/// As decisões do banco de volta como seção da ata — o mesmo texto que o
/// Copilot escreveu quando a reunião acabou.
///
/// Sem isto, regravar o `.md` a partir do banco (renomear um falante, o passe
/// final) apagava a seção: o banco tinha as decisões, a ata não.
pub(crate) fn decisions_markdown(rows: &[isper_core::store::StoredDecision]) -> Option<String> {
    let cards: Vec<isper_llm::CopilotCard> = rows
        .iter()
        .filter_map(|d| {
            let kind: isper_llm::CardKind =
                serde_json::from_value(serde_json::Value::String(d.kind.clone())).ok()?;
            let urgency: isper_llm::CardUrgency =
                serde_json::from_value(serde_json::Value::String(d.urgency.clone()))
                    .unwrap_or(isper_llm::CardUrgency::Medium);
            Some(isper_llm::CopilotCard {
                id: isper_llm::card_id(kind, &d.title),
                kind,
                title: d.title.clone(),
                description: d.description.clone(),
                owner: d.owner.clone(),
                due_date: d.due_date.clone(),
                urgency,
                at_secs: d.at_secs.max(0.0) as u32,
                status: isper_llm::CardStatus::Confirmed,
            })
        })
        .collect();
    isper_llm::render_decisions_markdown(&cards)
}

/// Converte os cards para as linhas que o banco guarda.
pub(crate) fn stored_decisions(
    cards: &[isper_llm::CopilotCard],
) -> Vec<isper_core::store::StoredDecision> {
    cards
        .iter()
        .map(|c| isper_core::store::StoredDecision {
            kind: c.kind.as_str().to_string(),
            title: c.title.clone(),
            description: c.description.clone(),
            owner: c.owner.clone(),
            due_date: c.due_date.clone(),
            urgency: c.urgency.as_str().to_string(),
            at_secs: c.at_secs as f32,
        })
        .collect()
}

fn ensure_copilot_loop(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.meeting_started.lock_or_recover().is_none() {
        return;
    }
    let mut cop = state.copilot.lock_or_recover();
    if cop.tx.is_some() {
        return;
    }
    let (tx, rx) = mpsc::channel::<CopilotCmd>();
    cop.tx = Some(tx);
    let my_gen = cop.generation;
    drop(cop);

    let app = app.clone();
    std::thread::spawn(move || {
        let interval = Duration::from_secs(COPILOT_AUTO_INTERVAL_SECS);
        // A primeira leitura sai cedo; depois o loop assume o ritmo normal.
        let mut next_pulse = Instant::now() + Duration::from_secs(FIRST_ROUND_SECS);
        let mut was_watching = false;
        loop {
            let asked = match rx.recv_timeout(WATCH_TICK) {
                Ok(CopilotCmd::Now) => true,
                Ok(CopilotCmd::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => false,
            };

            if app
                .state::<AppState>()
                .meeting_started
                .lock_or_recover()
                .is_none()
            {
                break;
            }

            let watching = copilot_window_on_screen(&app);
            {
                let state = app.state::<AppState>();
                let mut cop = state.copilot.lock_or_recover();
                if cop.generation != my_gen {
                    break; // outra reunião já é dona do estado
                }
                cop.watching = watching;
            }
            let opened = watching && !was_watching;
            was_watching = watching;

            match turn(asked, opened, watching, Instant::now() >= next_pulse) {
                Turn::Forced => {
                    analyze_copilot_step(&app, true, my_gen);
                    next_pulse = Instant::now() + interval;
                }
                // Lê a conversa agora, se houver o bastante. Sem isso, quem
                // abre no meio da reunião esperaria o próximo pulso com a tela
                // parada. Sem conversa nova, o pulso marcado continua valendo.
                Turn::Opened => {
                    if analyze_copilot_step(&app, false, my_gen) {
                        next_pulse = Instant::now() + interval;
                    }
                }
                Turn::Pulse => {
                    analyze_copilot_step(&app, false, my_gen);
                    next_pulse = Instant::now() + interval;
                }
                Turn::SkipPulse => next_pulse = Instant::now() + interval,
                Turn::Idle => {}
            }
        }

        // Só limpa se este ainda for o estado desta thread: outra reunião pode
        // já ter começado e instalado o `tx` dela.
        let state = app.state::<AppState>();
        let mut cop = state.copilot.lock_or_recover();
        if cop.generation != my_gen {
            return;
        }
        cop.tx = None;
        cop.running = false;
        drop(cop);
        emit_copilot(&app);
    });
}

/// A janela do Copilot está na tela: aberta, visível e não minimizada.
///
/// Atrás de outra janela conta como aberta — é assim que ela fica ao lado do
/// Teams. Escondida pelo atalho, fechada ou minimizada, não.
fn copilot_window_on_screen(app: &AppHandle) -> bool {
    app.get_webview_window("copilot")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
}

/// Conversa recente formatada para o prompt (`[mm:ss] Falante: texto`).
fn transcript_window(app: &AppHandle, window_secs: f32) -> String {
    let state = app.state::<AppState>();
    // Reunião encerrada e HUD ainda aberto: não há "agora" para recortar, e a
    // lista já está limitada a LIVE_KEEP — então entra tudo. (Com f32::MAX no
    // lugar do relógio, `f32::MAX - window_secs` continua f32::MAX e o filtro
    // descartava cada segmento: perguntar depois do fim dizia que não havia
    // transcrição nenhuma.)
    let cutoff = state
        .meeting_started
        .lock_or_recover()
        .map(|t| t.elapsed().as_secs_f32() - window_secs)
        .unwrap_or(f32::MIN);
    let live = state.live.lock_or_recover();
    live.iter()
        .filter(|s| !s.provisional && s.start_secs >= cutoff)
        .map(|s| {
            format!(
                "[{}] {}: {}",
                isper_core::meeting::fmt_ts(s.start_secs),
                s.speaker,
                s.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Procura, no índice semântico local, reuniões passadas que tratam do
/// assunto de agora.
///
/// Roda depois da rodada de análise, aproveitando o tópico que ela acabou de
/// resumir — uma frase curta e limpa é uma consulta muito melhor do que o
/// bloco de transcrição cru. Só sai da máquina o texto do tópico, e só se a
/// busca semântica estiver configurada; sem ela, o Copilot segue sem memória.
///
/// A reunião em curso não aparece aqui: ela só entra no índice depois de
/// salva, no fim.
fn recall_past_meetings(app: &AppHandle, topic: &str, my_gen: u64) {
    let topic = topic.trim();
    if topic.is_empty() {
        return;
    }
    let settings = isper_llm::load_settings();
    if !settings.embeddings.is_configured() {
        return;
    }
    let Ok(emb) = isper_llm::embedder_from_settings(&settings.embeddings) else {
        return;
    };

    let started = Instant::now();
    let hits = match emb.embed_query(topic) {
        Ok(vector) => match open_store() {
            Ok(store) => {
                let found = store
                    .semantic_search("meeting", &emb.id(), &vector, RECALL_LIMIT * 4)
                    .unwrap_or_default();
                let mut out: Vec<MemoryHit> = Vec::new();
                for hit in found {
                    if hit.score < RECALL_MIN_SCORE || out.len() >= RECALL_LIMIT {
                        continue;
                    }
                    // Uma memória por reunião: vários trechos da mesma
                    // conversa seriam três cards dizendo a mesma coisa.
                    if out.iter().any(|m| m.meeting_id == hit.ref_id) {
                        continue;
                    }
                    let Ok(Some(row)) = store.meeting_row(hit.ref_id) else {
                        continue;
                    };
                    out.push(MemoryHit {
                        id: format!("mem-{}", hit.ref_id),
                        meeting_id: hit.ref_id,
                        title: row.title,
                        started_at: row.started_at,
                        at_secs: hit.start_secs,
                        snippet: recall_snippet(&hit.text),
                        score: hit.score,
                    });
                }
                out
            }
            Err(e) => {
                tracing::warn!("copilot: memória sem banco: {e}");
                return;
            }
        },
        Err(e) => {
            tracing::warn!("copilot: não consegui consultar a memória: {e}");
            return;
        }
    };

    let state = app.state::<AppState>();
    let mut cop = state.copilot.lock_or_recover();
    if cop.generation != my_gen {
        return; // reunião anterior: o resultado não é desta conversa
    }
    let novas: Vec<MemoryHit> = hits
        .into_iter()
        .filter(|m| !cop.dismissed_memories.contains(&m.id))
        .collect();
    let mudou = novas != cop.memories;
    cop.memories = novas;
    cop.recalled_topic = Some(topic.to_string());
    drop(cop);
    if mudou {
        tracing::info!(
            secs = started.elapsed().as_secs_f32(),
            "copilot: memória atualizada"
        );
        emit_copilot(app);
    }
}

/// Trecho da reunião passada, curto o bastante para caber num card.
fn recall_snippet(text: &str) -> String {
    const MAX: usize = 220;
    let text = text.trim();
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Uma rodada de análise. Devolve se chegou a chamar a IA — sem conversa
/// nova o bastante, não chama.
fn analyze_copilot_step(app: &AppHandle, force: bool, my_gen: u64) -> bool {
    let state = app.state::<AppState>();
    if state.copilot.lock_or_recover().generation != my_gen {
        return false; // esta thread é de uma reunião que já acabou
    }
    let Some(started) = *state.meeting_started.lock_or_recover() else {
        return false;
    };
    let elapsed = started.elapsed().as_secs_f32();
    let total_chars: usize = state
        .live
        .lock_or_recover()
        .iter()
        .map(|s| s.text.len())
        .sum();

    {
        let cop = state.copilot.lock_or_recover();
        let minimo = if cop.seen_chars == 0 {
            MIN_NEW_CHARS_FIRST
        } else {
            MIN_NEW_CHARS_COPILOT
        };
        if !force && total_chars.saturating_sub(cop.seen_chars) < minimo {
            return false;
        }
    }

    let window = transcript_window(app, COPILOT_WINDOW_SECS);
    if window.trim().is_empty() {
        if force {
            let mut cop = state.copilot.lock_or_recover();
            cop.error = Some("ainda não há falas na reunião para analisar".into());
            drop(cop);
            emit_copilot(app);
        }
        return false;
    }

    let settings = isper_llm::load_settings();
    let provider = match isper_llm::provider_from_settings(&settings) {
        Ok(p) => p,
        Err(e) => {
            let mut cop = state.copilot.lock_or_recover();
            cop.error = Some(match e {
                isper_llm::LlmError::NotConfigured => {
                    "sem provider de IA configurado — defina em Configurações → Inteligência"
                        .to_string()
                }
                other => other.to_string(),
            });
            drop(cop);
            emit_copilot(app);
            return false;
        }
    };

    let previous_cards = {
        let mut cop = state.copilot.lock_or_recover();
        if cop.generation != my_gen {
            return false;
        }
        cop.running = true;
        cop.error = None;
        cop.last_round = Some(Instant::now());
        cop.cards.clone()
    };
    emit_copilot(app);

    let mut recall: Option<String> = None;
    let started_at = Instant::now();
    let outcome = isper_llm::analyze_meeting(
        provider.as_ref(),
        &isper_llm::CopilotInput {
            window: &window,
            elapsed: &isper_core::meeting::fmt_ts(elapsed),
            elapsed_secs: elapsed as u32,
            previous_cards: &previous_cards,
        },
    );

    {
        let mut cop = state.copilot.lock_or_recover();
        // A chamada demorou; a reunião pode ter acabado (e outra começado)
        // nesse meio-tempo. O resultado é da reunião anterior: descarta.
        if cop.generation != my_gen {
            tracing::debug!("copilot: análise de uma reunião anterior descartada");
            return true;
        }
        cop.running = false;
        cop.last_round = Some(Instant::now());
        match outcome {
            Ok(analysis) => {
                let novos = cop.merge_cards(analysis.cards);
                tracing::info!(
                    secs = started_at.elapsed().as_secs_f32(),
                    novos,
                    "copilot: análise concluída via {}",
                    provider.name()
                );
                cop.active_topic = analysis.current_topic;
                if let Some(dyn_note) = analysis.dynamics_note {
                    cop.dynamics_note = Some(dyn_note);
                }
                cop.seen_chars = total_chars;
                cop.last_updated = Some(chrono::Local::now().format("%H:%M:%S").to_string());
                cop.error = None;
                // Tópico novo ⇒ vale procurar na memória. Repetir a mesma
                // consulta a cada rodada só gastaria embeddings.
                if cop.recalled_topic.as_deref() != Some(cop.active_topic.as_str()) {
                    recall = Some(cop.active_topic.clone());
                }
            }
            Err(e) => {
                tracing::warn!("copilot: análise falhou: {e}");
                cop.error = Some(e.to_string());
            }
        }
    }
    emit_copilot(app);

    if let Some(topic) = recall {
        recall_past_meetings(app, &topic, my_gen);
    }
    true
}

// ---------------------------------------------------------------- Comandos Tauri

#[tauri::command]
pub(crate) fn copilot_get_state(app: AppHandle) -> CopilotDto {
    copilot_dto(&app)
}

#[tauri::command]
pub(crate) fn copilot_analyze_now(app: AppHandle) -> Result<(), String> {
    if app
        .state::<AppState>()
        .meeting_started
        .lock_or_recover()
        .is_none()
    {
        return Err("nenhuma reunião em andamento".into());
    }
    if !copilot_configured() {
        return Err(
            "sem provider de IA configurado — defina em Configurações → Inteligência".into(),
        );
    }
    ensure_copilot_loop(&app);
    let state = app.state::<AppState>();
    let cop = state.copilot.lock_or_recover();
    match cop.tx.as_ref() {
        Some(tx) => tx
            .send(CopilotCmd::Now)
            .map_err(|_| "loop do copilot parado".to_string()),
        None => Err("não consegui acionar o copilot".into()),
    }
}

#[tauri::command]
pub(crate) fn copilot_card_action(
    app: AppHandle,
    card_id: String,
    action: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut cop = state.copilot.lock_or_recover();
    cop.apply_card_action(&card_id, &action)
        .map_err(ToString::to_string)?;
    drop(cop);
    emit_copilot(&app);
    Ok(())
}

/// Dispensa uma memória: some do feed e não volta nesta reunião.
#[tauri::command]
pub(crate) fn copilot_dismiss_memory(app: AppHandle, memory_id: String) {
    let state = app.state::<AppState>();
    let mut cop = state.copilot.lock_or_recover();
    cop.memories.retain(|m| m.id != memory_id);
    if !cop.dismissed_memories.contains(&memory_id) {
        cop.dismissed_memories.push(memory_id);
    }
    drop(cop);
    emit_copilot(&app);
}

/// Guarda o bloco de notas. Depois que a reunião foi salva, a edição vai
/// direto para ela — banco e ata — e a resposta é `true`, para a tela dizer
/// "salvo na reunião".
#[tauri::command]
pub(crate) fn copilot_save_scratchpad(app: AppHandle, text: String) -> Result<bool, String> {
    let state = app.state::<AppState>();
    // Travado até o arquivo ser gravado: duas edições seguidas não podem
    // chegar ao disco fora de ordem, nem ser atropeladas pelo fim da reunião
    // (`attach_saved_meeting` trava o mesmo estado).
    let mut cop = state.copilot.lock_or_recover();
    cop.scratchpad = text;
    let Some(id) = cop.saved_meeting else {
        return Ok(false);
    };
    let store = open_store().map_err(|e| e.to_string())?;
    store
        .set_notes(id, &cop.scratchpad)
        .map_err(|e| e.to_string())?;
    rewrite_markdown(&store, id);
    Ok(true)
}

/// Mantém o HUD por cima do Teams/Zoom (modo sidecar).
#[tauri::command]
pub(crate) fn copilot_set_always_on_top(app: AppHandle, on: bool) -> Result<(), String> {
    let win = app
        .get_webview_window("copilot")
        .ok_or("a janela do Copilot não está aberta")?;
    win.set_always_on_top(on).map_err(|e| e.to_string())
}

/// Um pedaço de resposta a caminho da tela.
#[derive(Clone, serde::Serialize)]
pub(crate) struct CopilotTokenDto {
    /// Quem pediu — a tela descarta pedaços de uma pergunta que já saiu de cena.
    pub(crate) id: String,
    pub(crate) chunk: String,
}

/// Junta os pedaços do modelo e os manda para a tela em lotes.
struct TokenPump<'a> {
    app: &'a AppHandle,
    id: &'a str,
    buf: String,
    last: Instant,
}

impl<'a> TokenPump<'a> {
    fn new(app: &'a AppHandle, id: &'a str) -> Self {
        Self {
            app,
            id,
            buf: String::new(),
            last: Instant::now(),
        }
    }

    fn push(&mut self, chunk: &str) {
        self.buf.push_str(chunk);
        if self.last.elapsed() >= STREAM_FLUSH_EVERY {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let _ = self.app.emit(
            "isper-copilot-token",
            CopilotTokenDto {
                id: self.id.to_string(),
                chunk: std::mem::take(&mut self.buf),
            },
        );
        self.last = Instant::now();
    }
}

#[tauri::command]
pub(crate) async fn copilot_query(
    app: AppHandle,
    question: String,
    stream_id: String,
) -> Result<String, String> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err("escreva a pergunta primeiro".into());
    }
    // Rede bloqueante nunca no thread do comando: congelaria a janela inteira.
    tauri::async_runtime::spawn_blocking(move || {
        let transcript = transcript_window(&app, QA_WINDOW_SECS);
        if transcript.trim().is_empty() {
            return Err("ainda não há transcrição disponível na reunião".to_string());
        }
        let settings = isper_llm::load_settings();
        let provider = isper_llm::provider_from_settings(&settings).map_err(|e| e.to_string())?;

        let mut pump = TokenPump::new(&app, &stream_id);
        let out = isper_llm::query_meeting_stream(
            provider.as_ref(),
            &transcript,
            &question,
            &mut |chunk| pump.push(chunk),
        );
        // O resto do buffer sai mesmo se a chamada falhou no meio: o usuário
        // fica com o que o modelo chegou a dizer.
        pump.flush();
        out.map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn copilot_enrich_notes(
    app: AppHandle,
    raw_notes: String,
    stream_id: String,
) -> Result<String, String> {
    if raw_notes.trim().is_empty() {
        return Err("escreva algumas anotações primeiro".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let transcript = transcript_window(&app, QA_WINDOW_SECS);
        if transcript.trim().is_empty() {
            return Err("ainda não há transcrição disponível na reunião".to_string());
        }
        let settings = isper_llm::load_settings();
        let provider = isper_llm::provider_from_settings(&settings).map_err(|e| e.to_string())?;

        let mut pump = TokenPump::new(&app, &stream_id);
        let out = isper_llm::enrich_notes_stream(
            provider.as_ref(),
            &transcript,
            &raw_notes,
            &mut |chunk| pump.push(chunk),
        );
        pump.flush();
        out.map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use isper_llm::{CardKind, CardStatus, CardUrgency, CopilotCard, card_id};

    fn card(kind: CardKind, title: &str) -> CopilotCard {
        CopilotCard {
            id: card_id(kind, title),
            kind,
            title: title.into(),
            description: "detalhe".into(),
            owner: None,
            due_date: None,
            urgency: CardUrgency::Medium,
            at_secs: 10,
            status: CardStatus::Proposed,
        }
    }

    #[test]
    fn card_actions_modificam_ou_removem_cards() {
        let mut state = CopilotAppState::default();
        state.cards.push(card(CardKind::Decision, "Alinhar prazos"));
        let id = state.cards[0].id.clone();

        assert!(state.apply_card_action(&id, "confirm").is_ok());
        assert_eq!(state.cards[0].status, CardStatus::Confirmed);

        assert!(state.apply_card_action(&id, "discard").is_ok());
        assert_eq!(state.cards[0].status, CardStatus::Discarded);

        // Desfazer devolve para a fila.
        assert!(state.apply_card_action(&id, "reset").is_ok());
        assert_eq!(state.cards[0].status, CardStatus::Proposed);

        assert!(state.apply_card_action(&id, "delete").is_ok());
        assert!(state.cards.is_empty());

        assert_eq!(
            state.apply_card_action(&id, "confirm"),
            Err("card não encontrado")
        );
        assert_eq!(
            state.apply_card_action("nada", "voar"),
            Err("card não encontrado")
        );
    }

    #[test]
    fn acao_invalida_em_card_existente_e_recusada() {
        let mut state = CopilotAppState::default();
        state.cards.push(card(CardKind::Decision, "Alinhar prazos"));
        let id = state.cards[0].id.clone();
        assert_eq!(state.apply_card_action(&id, "voar"), Err("ação inválida"));
        assert_eq!(state.cards[0].status, CardStatus::Proposed);
    }

    #[test]
    fn merge_nao_duplica_o_mesmo_assunto_e_atualiza_detalhes() {
        let mut state = CopilotAppState::default();
        assert_eq!(
            state.merge_cards(vec![card(CardKind::Decision, "Lançar dia 30")]),
            1
        );

        // Mesma decisão, reformulada e com detalhe novo.
        let mut de_novo = card(CardKind::Decision, "Lancar dia 30");
        de_novo.description = "Cliente confirmou por e-mail".into();
        de_novo.at_secs = 900;
        assert_eq!(state.merge_cards(vec![de_novo]), 0, "não deveria duplicar");
        assert_eq!(state.cards.len(), 1);
        assert_eq!(state.cards[0].description, "Cliente confirmou por e-mail");
        assert_eq!(
            state.cards[0].at_secs, 10,
            "a hora é a do primeiro avistamento"
        );

        // Assunto diferente entra como card novo.
        assert_eq!(
            state.merge_cards(vec![card(CardKind::Action, "Enviar contrato")]),
            1
        );
        assert_eq!(state.cards.len(), 2);
    }

    #[test]
    fn merge_preserva_o_que_o_usuario_ja_decidiu() {
        let mut state = CopilotAppState::default();
        state.merge_cards(vec![card(CardKind::Decision, "Lançar dia 30")]);
        let id = state.cards[0].id.clone();
        state.apply_card_action(&id, "confirm").unwrap();

        let mut regressao = card(CardKind::Decision, "Lançar dia 30");
        regressao.description = "texto novo da IA".into();
        state.merge_cards(vec![regressao]);

        assert_eq!(
            state.cards[0].status,
            CardStatus::Confirmed,
            "confirmação não pode se perder"
        );
        assert_eq!(
            state.cards[0].description, "detalhe",
            "card validado não é sobrescrito"
        );

        // O mesmo vale para descartado: a IA repete o ponto toda rodada.
        state.apply_card_action(&id, "discard").unwrap();
        state.merge_cards(vec![card(CardKind::Decision, "Lançar dia 30")]);
        assert_eq!(state.cards.len(), 1);
        assert_eq!(state.cards[0].status, CardStatus::Discarded);
    }

    #[test]
    fn recorte_da_memoria_cabe_no_card() {
        let curto = "Ficou combinado o preço de quarenta mil.";
        assert_eq!(recall_snippet(curto), curto, "texto curto passa inteiro");

        let longo = "a".repeat(500);
        let cortado = recall_snippet(&longo);
        assert!(cortado.chars().count() <= 220);
        assert!(cortado.ends_with('…'), "corte precisa ser visível");

        assert_eq!(recall_snippet("   sobra espaço   "), "sobra espaço");
    }

    #[test]
    fn so_o_que_foi_confirmado_vira_linha_do_banco() {
        let mut confirmado = card(CardKind::Action, "Enviar proposta");
        confirmado.status = CardStatus::Confirmed;
        confirmado.owner = Some("Eu".into());
        confirmado.due_date = Some("Sexta".into());
        confirmado.urgency = CardUrgency::High;
        confirmado.at_secs = 53;

        let rows = stored_decisions(std::slice::from_ref(&confirmado));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "action", "o tipo vai como texto para o banco");
        assert_eq!(rows[0].urgency, "high");
        assert_eq!(rows[0].owner.as_deref(), Some("Eu"));
        assert_eq!(rows[0].at_secs, 53.0);
        assert_eq!(rows[0].title, "Enviar proposta");
    }

    #[test]
    fn dispensar_memoria_tira_do_feed_e_anota_para_nao_voltar() {
        let memorias = vec![
            MemoryHit {
                id: "mem-7".into(),
                meeting_id: 7,
                title: "Negociação anterior".into(),
                started_at: "10/09/2026 15:30".into(),
                at_secs: Some(412.0),
                snippet: "preço de quarenta mil".into(),
                score: 0.71,
            },
            MemoryHit {
                id: "mem-3".into(),
                meeting_id: 3,
                title: "Retrospectiva".into(),
                started_at: "02/09/2026 09:00".into(),
                at_secs: None,
                snippet: "relatórios atrasaram".into(),
                score: 0.62,
            },
        ];
        let mut state = CopilotAppState {
            memories: memorias,
            ..CopilotAppState::default()
        };

        // O que o comando faz com o lock na mão.
        let alvo = "mem-7".to_string();
        state.memories.retain(|m| m.id != alvo);
        state.dismissed_memories.push(alvo.clone());

        assert_eq!(state.memories.len(), 1);
        assert_eq!(state.memories[0].id, "mem-3");
        assert!(state.dismissed_memories.contains(&alvo));

        // E a próxima busca filtra por essa lista: a dispensada não volta.
        let nova_busca = ["mem-7".to_string(), "mem-3".to_string()];
        let filtrada: Vec<&String> = nova_busca
            .iter()
            .filter(|id| !state.dismissed_memories.contains(id))
            .collect();
        assert_eq!(filtrada, vec![&"mem-3".to_string()]);
    }

    #[test]
    fn geracao_distingue_o_estado_de_cada_reuniao() {
        // O que reset_copilot faz com o lock na mão: avança a geração e zera
        // o resto. Uma thread nascida na geração anterior tem de perceber.
        let mut state = CopilotAppState::default();
        state.cards.push(card(CardKind::Decision, "Da reunião A"));
        let antes = state.generation;

        let next = state.generation.wrapping_add(1);
        state = CopilotAppState::default();
        state.generation = next;

        assert_ne!(state.generation, antes, "a geração precisa avançar");
        assert!(state.cards.is_empty(), "a reunião nova começa limpa");
    }

    #[test]
    fn com_a_janela_fechada_o_pulso_nao_chama_a_ia() {
        // (asked, opened, watching, pulse_due)
        assert_eq!(turn(false, false, false, true), Turn::SkipPulse);
        assert_eq!(turn(false, false, false, false), Turn::Idle);
        assert_eq!(turn(false, false, true, true), Turn::Pulse);
        assert_eq!(turn(false, false, true, false), Turn::Idle);
    }

    #[test]
    fn abrir_a_janela_le_a_conversa_na_hora() {
        assert_eq!(turn(false, true, true, false), Turn::Opened);
        // Mesmo que o pulso também esteja vencido: uma leitura só.
        assert_eq!(turn(false, true, true, true), Turn::Opened);
    }

    #[test]
    fn pedido_explicito_sempre_roda() {
        for (opened, watching, due) in [
            (false, true, false),
            (true, true, true),
            (false, false, false),
        ] {
            assert_eq!(turn(true, opened, watching, due), Turn::Forced);
        }
    }

    #[test]
    fn notas_da_reuniao_que_acabou_seguem_o_bloco_ate_outra_comecar() {
        let mut state = CopilotAppState {
            scratchpad: "- valor: 40 mil".into(),
            ..CopilotAppState::default()
        };
        let wrap = CopilotWrapUp {
            generation: state.generation,
            decisions: Vec::new(),
            notes: state.scratchpad.clone(),
        };

        // Ainda escrevendo enquanto a transcrição termina: vale o bloco atual.
        state.scratchpad.push_str("\n- prazo: sexta");
        assert_eq!(state.notes_for(&wrap), "- valor: 40 mil\n- prazo: sexta");

        // Outra reunião começou e zerou o bloco: vale o que havia ao encerrar.
        let next = state.generation.wrapping_add(1);
        state = CopilotAppState::default();
        state.generation = next;
        assert_eq!(state.notes_for(&wrap), "- valor: 40 mil");
        assert_eq!(
            state.saved_meeting, None,
            "a reunião nova não herda a ligação com a anterior"
        );
    }

    #[test]
    fn decisoes_do_banco_voltam_para_a_ata_com_o_mesmo_texto() {
        let mut acao = card(CardKind::Action, "Enviar proposta");
        acao.status = CardStatus::Confirmed;
        acao.owner = Some("Eu".into());
        acao.due_date = Some("Sexta".into());
        acao.urgency = CardUrgency::High;
        let mut decisao = card(CardKind::Decision, "Lançar dia 30");
        decisao.status = CardStatus::Confirmed;
        let cards = vec![decisao, acao];

        let ao_encerrar = isper_llm::render_decisions_markdown(&cards).unwrap();
        let do_banco = decisions_markdown(&stored_decisions(&cards)).unwrap();
        assert_eq!(
            do_banco, ao_encerrar,
            "regravar pelo banco não pode mudar a seção"
        );

        // Um tipo que este ISPer não conhece é deixado de fora, sem derrubar o resto.
        let mut linhas = stored_decisions(&cards);
        linhas[0].kind = "algo-novo".into();
        let parcial = decisions_markdown(&linhas).unwrap();
        assert!(parcial.contains("Enviar proposta"));
        assert!(!parcial.contains("Lançar dia 30"));
    }

    #[test]
    fn o_titulo_das_decisoes_e_o_que_o_import_procura() {
        // O `isper-llm` escreve a seção e o `isper-core` a reconhece; os dois
        // não se conhecem, então é aqui, onde os dois estão, que se confere.
        let mut c = card(CardKind::Decision, "X");
        c.status = CardStatus::Confirmed;
        let md = isper_llm::render_decisions_markdown(&[c]).unwrap();
        assert!(
            md.starts_with(&format!(
                "{}\n",
                isper_core::meeting::COPILOT_DECISIONS_HEADING
            )),
            "{md}"
        );
    }

    #[test]
    fn dinamica_de_fala_acumula_e_detecta_monologo() {
        let mut state = CopilotAppState::default();
        state.note_speech("Eu", 60.0);
        state.note_speech("Participante 1", 30.0);
        assert_eq!(state.me_secs, 60.0);
        assert_eq!(state.others_secs, 30.0);
        assert!(!state.monologue());

        // Fala contínua de "Eu" passando do limite acende o aviso.
        state.note_speech("Eu", MONOLOGUE_SECS);
        assert!(state.monologue());

        // Alguém interrompeu: a sequência zera (o total, não).
        state.note_speech("Participante 2", 5.0);
        assert!(!state.monologue());
        assert_eq!(state.me_secs, 60.0 + MONOLOGUE_SECS);
    }

    #[test]
    fn monologo_olha_a_sequencia_e_nao_o_total() {
        // Muita fala de "Eu", mas sempre intercalada: não é monólogo.
        let mut state = CopilotAppState::default();
        for _ in 0..10 {
            state.note_speech("Eu", 60.0);
            state.note_speech("Participante 1", 1.0);
        }
        assert!(state.me_secs > MONOLOGUE_SECS);
        assert!(!state.monologue());
    }
}
