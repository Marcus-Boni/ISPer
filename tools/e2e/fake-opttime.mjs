// Um OptTime de mentira para os e2e das rotinas, da agenda e do assistente
// (Fases 10.2 a 10.4): o MCP hospedado
// sem sessão, como o de produção (POST com JSON-RPC, GET recusado com 405,
// erro de ferramenta com o código em _meta["opt-time/error"]), com um dia que
// começa em 4h40 de 8h e as sugestões que fecham o dia.
//
//   node tools/e2e/fake-opttime.mjs <porta> <token>
//
// GET /calls devolve as chamadas de ferramenta recebidas (para o roteiro
// conferir o que o ISPer mandou). Só escuta em 127.0.0.1.
import http from 'node:http';

const port = Number(process.argv[2] || 0);
const token = process.argv[3] || 'opt_tok_e2e';
const calls = [];
const applied = new Map();
let total = 280;
const TARGET = 480;

const today = () => {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
};
const label = (m) => (m % 60 ? `${Math.floor(m / 60)}h${String(m % 60).padStart(2, '0')}` : `${m / 60}h`);

const SUGGESTIONS = [
  { id: 'sug-1', source: 'calendar', sourceRef: 'evt-1', projectId: 'p1', projectName: 'Portal do Cliente',
    description: 'Refinamento do backlog', durationMinutes: 120, billable: true, confidence: 'high',
    evidence: 'Reunião aceita no Outlook, 2h' },
  { id: 'sug-2', source: 'work_item', sourceRef: '4512', projectId: 'p1', projectName: 'Portal do Cliente',
    description: 'Ajuste no filtro de pedidos (#4512)', durationMinutes: 80, billable: true, confidence: 'medium',
    evidence: 'Work item ativo atribuído a você' },
  { id: 'sug-3', source: 'voice_memo', sourceRef: null, projectId: null, projectName: null,
    description: 'Algo de uma fonte que o ISPer não conhece', durationMinutes: 30, billable: false, confidence: 'low',
    evidence: 'Fonte nova' },
];

// Um instante daqui a `min` minutos, em ISO com o offset local (como o OptTime manda).
function isoIn(min) {
  const d = new Date(Date.now() + min * 60_000);
  d.setSeconds(0, 0);
  const p = (n) => String(Math.abs(n)).padStart(2, '0');
  const off = -d.getTimezoneOffset();
  const sign = off >= 0 ? '+' : '-';
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}:00${sign}${p(Math.trunc(off / 60))}:${p(off % 60)}`;
}

// A agenda do dia, relativa à hora do teste: uma que já passou, uma reunião acontecendo, uma
// daqui a 30 min (fora da janela do aviso de preparo), uma presencial mais
// tarde, um bloqueio livre e um feriado de dia inteiro.
function agendaEvents() {
  const ev = (uid, subject, from, to, extra = {}) => ({
    id: `id-${uid}`, iCalUId: uid, seriesMasterId: null, type: 'singleInstance', subject,
    start: isoIn(from), end: isoIn(to), durationMinutes: to - from, isAllDay: false, isOnline: true,
    joinUrl: `https://teams.microsoft.com/l/meetup-join/${uid}`,
    organizer: { name: 'Ana Souza', email: 'ana@example.com' }, isOrganizer: false,
    responseStatus: 'accepted', attendeeCount: 4, attendees: [], location: null, showAs: 'busy',
    sensitivity: 'normal', webLink: null, loggedMinutes: 0, attendance: null, ...extra,
  });
  return [
    ev('ev-feriado', 'Feriado municipal', -600, 840, { isAllDay: true, isOnline: false, joinUrl: null, showAs: 'oof' }),
    ev('ev-cedo', 'Alinhamento da manhã', -120, -60),
    ev('ev-daily', 'Daily do Portal', -20, 40, { seriesMasterId: 'serie-daily', type: 'occurrence' }),
    ev('ev-refino', 'Refinamento do backlog', 30, 90),
    ev('ev-foco', 'Bloqueio de foco', 100, 160, { showAs: 'free', isOnline: false, joinUrl: null, attendeeCount: 1 }),
    ev('ev-visita', 'Visita ao cliente', 180, 240, { isOnline: false, joinUrl: null, location: 'Salvador' }),
  ];
}

// O catálogo que o assistente vê (Fase 10.4): duas de leitura, uma que lança
// e uma que apaga (essa nunca fica livre nas permissões).
const TOOLS = [
  { name: 'opt_time_get_today_summary', title: 'Resumo do dia', description: 'Horas lançadas num dia e quanto falta.',
    inputSchema: { type: 'object', properties: { date: { type: 'string', description: 'AAAA-MM-DD' } } },
    annotations: { readOnlyHint: true, destructiveHint: false } },
  { name: 'opt_time_get_my_agenda', title: 'Minha agenda', description: 'Os eventos do Outlook de um dia.',
    inputSchema: { type: 'object', properties: { date: { type: 'string' }, days: { type: 'integer' } } },
    annotations: { readOnlyHint: true, destructiveHint: false } },
  { name: 'opt_time_log_time', title: 'Lançar horas', description: 'Lança um apontamento no dia.',
    inputSchema: { type: 'object', properties: { projectId: { type: 'string' }, durationMinutes: { type: 'integer' }, description: { type: 'string' } }, required: ['durationMinutes'] },
    annotations: { readOnlyHint: false, destructiveHint: false } },
  { name: 'opt_time_delete_entry', title: 'Apagar lançamento', description: 'Apaga um apontamento.',
    inputSchema: { type: 'object', properties: { entryId: { type: 'string' } }, required: ['entryId'] },
    annotations: { readOnlyHint: false, destructiveHint: true } },
];

const ok = (data) => ({ content: [{ type: 'text', text: 'ok' }], structuredContent: data });
const fail = (code, message, hint) => ({
  content: [{ type: 'text', text: `❌ ${message}` }],
  _meta: { 'opt-time/error': { code, message, hint: hint ?? null, details: null } },
  isError: true,
});

function tool(name, args) {
  const date = args.date || today();
  switch (name) {
    case 'opt_time_whoami':
      return ok({
        userId: 'u-e2e', name: 'Fulano de Teste', email: 'fulano@example.com', role: 'member',
        scopes: ['time:read', 'time:write', 'calendar:read'], tokenName: 'ISPer e2e',
        timezone: 'America/Sao_Paulo', weeklyCapacityMinutes: 2400,
        today: { date: today(), totalMinutes: total, dailyCapacityMinutes: TARGET },
        microsoft: { connected: true, needsReconnect: false, tokenUsable: true },
        azureDevOps: { configured: true }, eveningDigestEnabled: true,
      });
    case 'opt_time_get_today_summary':
      return ok({
        date, weekday: 'hoje', totalMinutes: total, totalLabel: label(total), billableMinutes: total,
        entryCount: 2, dailyCapacityMinutes: TARGET, remainingMinutes: Math.max(0, TARGET - total),
        remainingLabel: label(Math.max(0, TARGET - total)), isComplete: total >= TARGET,
        byProject: [], entries: [], activeTimer: null, weekTotalMinutes: total,
        weeklyCapacityMinutes: 2400, isWorkday: true, targetMinutes: TARGET, warnings: [],
      });
    case 'opt_time_suggest_daily_entries':
      return ok({
        date, suggestions: total >= TARGET ? [] : SUGGESTIONS.map((s) => ({ ...s, date, durationLabel: label(s.durationMinutes), reasons: [s.evidence] })),
        alreadyLoggedMinutes: total, alreadyLoggedLabel: label(total), targetMinutes: TARGET,
        gapMinutes: Math.max(0, TARGET - total),
        sources: { outlook: true, teamsCalls: true, azureDevOps: true, history: false, commits: 0 },
        warnings: [], notes: [],
      });
    case 'opt_time_get_my_agenda':
      return ok({
        timezone: 'America/Sao_Paulo', range: { start: isoIn(-720), end: isoIn(720) },
        sources: { outlook: true }, warnings: [], events: agendaEvents(),
      });
    case 'opt_time_apply_suggestions': {
      if (applied.has(args.idempotencyKey)) return ok({ ...applied.get(args.idempotencyKey), replayed: true });
      const items = Array.isArray(args.items) ? args.items : [];
      for (const it of items) {
        const s = SUGGESTIONS.find((x) => x.id === it.suggestionId);
        if (!s) return fail('VALIDATION_ERROR', `Sugestão desconhecida: ${it.suggestionId}`);
        if (!s.projectId && !it.projectId) return fail('VALIDATION_ERROR', 'Sugestão sem projeto', 'Informe projectId.');
      }
      for (const it of items) total += it.durationMinutes ?? SUGGESTIONS.find((x) => x.id === it.suggestionId).durationMinutes;
      const out = {
        date, createdEntryIds: items.map((_, i) => `e${calls.length}-${i}`), dayTotalMinutes: total,
        dailyCapacityMinutes: TARGET, remainingMinutes: Math.max(0, TARGET - total), replayed: false,
      };
      applied.set(args.idempotencyKey, out);
      return ok(out);
    }
    default:
      return fail('MICROSOFT_NOT_CONNECTED', 'Conta Microsoft não conectada.', 'Entre no OptTime com a conta Microsoft.');
  }
}

function answer(msg) {
  switch (msg.method) {
    case 'initialize':
      return { protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'opt-time', version: 'e2e' } };
    case 'tools/list':
      return { tools: TOOLS };
    case 'tools/call': {
      const { name, arguments: args = {} } = msg.params || {};
      calls.push({ name, args });
      return tool(name, args);
    }
    default:
      return null;
  }
}

const server = http.createServer((req, res) => {
  if (req.method === 'GET' && req.url === '/calls') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(calls));
    return;
  }
  if (req.method === 'GET') { res.writeHead(405).end(); return; }
  if (req.method === 'DELETE') { res.writeHead(204).end(); return; }
  if (req.method !== 'POST') { res.writeHead(400).end(); return; }
  if (req.headers.authorization !== `Bearer ${token}`) {
    res.writeHead(401, { 'Content-Type': 'application/json', 'WWW-Authenticate': 'Bearer realm="opt-time"' });
    res.end(JSON.stringify({ error: { code: 'UNAUTHORIZED', message: 'token inválido' } }));
    return;
  }
  let body = '';
  req.on('data', (c) => { body += c; });
  req.on('end', () => {
    let msg;
    try { msg = JSON.parse(body); } catch { res.writeHead(400).end(); return; }
    if (msg.id === undefined) { res.writeHead(202).end(); return; }
    const result = answer(msg);
    const reply = result === null
      ? { jsonrpc: '2.0', id: msg.id, error: { code: -32601, message: `método desconhecido: ${msg.method}` } }
      : { jsonrpc: '2.0', id: msg.id, result };
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(reply));
  });
});

server.listen(port, '127.0.0.1', () => {
  console.log(`fake-opttime em http://127.0.0.1:${server.address().port}/api/mcp`);
});
