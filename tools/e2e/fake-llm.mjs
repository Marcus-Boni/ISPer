// Uma IA de mentira, compatível com a API da OpenAI (chat/completions), para
// o e2e do assistente (Fase 10.4): o ISPer fala com ela pelo provedor
// "Compatível com OpenAI" das Configurações, sem chave e sem rede.
//
//   node tools/e2e/fake-llm.mjs <porta>
//
// O roteiro é fixo e só usa as ferramentas que o ISPer ofereceu na chamada:
//
// - "hoje" / "tenho" / "resumo da minha manhã" → tarefas_do_dia;
// - "lembr" → lembrar (a preferência por reuniões depois das 10h);
// - "cria" → criar_tarefa ("Ligar pro João", hoje às 15:00);
// - "horas" / "Feche o meu dia" → opt_time_get_today_summary;
// - com os resultados na mão, responde em tópicos citando cada [[ref]] que
//   veio deles; recusa vira "Tudo bem, não fiz.".
//
// Ferramenta pedida que não veio na lista (bloqueada nas permissões) vira a
// resposta "Essa ferramenta está bloqueada.". GET /requests devolve o resumo
// de cada chamada recebida. Só escuta em 127.0.0.1.
import http from 'node:http';

const port = Number(process.argv[2] || 0);
const requests = [];
let nextCall = 1;

const today = () => {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
};

// Os pares (ref, título) num resultado de ferramenta, em qualquer profundidade.
function refsIn(value, out = []) {
  if (Array.isArray(value)) { value.forEach((v) => refsIn(v, out)); return out; }
  if (value && typeof value === 'object') {
    if (typeof value.ref === 'string') {
      const title = value.titulo || value.title || value.texto || value.resultado?.totalLabel || value.ref;
      out.push({ ref: value.ref, title: typeof title === 'string' ? title : value.ref });
    }
    for (const v of Object.values(value)) refsIn(v, out);
  }
  return out;
}

function plan(text) {
  const t = text.toLowerCase();
  if (t.includes('lembr')) {
    return [['lembrar', { texto: 'Prefiro reuniões depois das 10h', tipo: 'preferencia' }]];
  }
  if (t.includes('cria')) {
    return [['criar_tarefa', { titulo: 'Ligar pro João', dia: today(), hora: '15:00', prazo: null }]];
  }
  if (t.includes('horas') || t.includes('feche o meu dia')) {
    return [['opt_time_get_today_summary', { date: today() }], ['tarefas_do_dia', { dia: null }]];
  }
  if (t.includes('hoje') || t.includes('tenho') || t.includes('resumo da minha manh')) {
    return [['tarefas_do_dia', { dia: null }]];
  }
  return [];
}

function reply(body) {
  const messages = body.messages || [];
  const offered = (body.tools || []).map((t) => t.function?.name).filter(Boolean);
  const lastUser = [...messages].reverse().find((m) => m.role === 'user');
  const question = typeof lastUser?.content === 'string' ? lastUser.content : '';
  const sinceUser = messages.slice(messages.lastIndexOf(lastUser) + 1);
  const results = sinceUser.filter((m) => m.role === 'tool');
  requests.push({
    tools: offered,
    question,
    system: (messages.find((m) => m.role === 'system') || {}).content || '',
    results: results.map((m) => String(m.content).slice(0, 400)),
  });

  if (!results.length) {
    const wanted = plan(question);
    if (!wanted.length) return { content: 'Não sei responder isso no teste.' };
    const calls = wanted.filter(([name]) => offered.includes(name));
    if (!calls.length) return { content: 'Essa ferramenta está bloqueada.' };
    return {
      content: null,
      tool_calls: calls.map(([name, args]) => ({
        id: `call_${nextCall++}`, type: 'function', function: { name, arguments: JSON.stringify(args) },
      })),
    };
  }

  const refused = results.filter((m) => /não autorizou|seguiu com outra pergunta|bloqueada/.test(String(m.content)));
  if (refused.length === results.length) return { content: 'Tudo bem, não fiz.' };
  const found = [];
  for (const m of results) {
    let parsed;
    try { parsed = JSON.parse(m.content); } catch { continue; }
    for (const r of refsIn(parsed)) if (!found.some((f) => f.ref === r.ref)) found.push(r);
  }
  if (!found.length) return { content: 'Não achei nada.' };
  const created = results.some((m) => String(m.content).includes('"criada"'));
  const saved = results.some((m) => String(m.content).includes('"guardada"'));
  const head = saved ? 'Guardei na memória:' : created ? 'Criei a tarefa:' : 'Encontrei:';
  return { content: [head, ...found.map((f) => `- **${f.title}** [[${f.ref}]]`)].join('\n') };
}

const server = http.createServer((req, res) => {
  if (req.method === 'GET' && req.url === '/requests') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(requests));
    return;
  }
  if (req.method === 'GET' && req.url === '/v1/models') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ object: 'list', data: [{ id: 'fake-agente', object: 'model' }] }));
    return;
  }
  if (req.method !== 'POST' || req.url !== '/v1/chat/completions') { res.writeHead(404).end(); return; }
  let raw = '';
  req.on('data', (c) => { raw += c; });
  req.on('end', () => {
    let body;
    try { body = JSON.parse(raw); } catch { res.writeHead(400).end(); return; }
    // Sem ferramentas (resumos, títulos): um texto curto basta.
    const message = body.tools ? reply(body) : { content: 'ok' };
    if (body.stream) {
      res.writeHead(200, { 'Content-Type': 'text/event-stream' });
      res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: { content: message.content || '' } }] })}\n\n`);
      res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] })}\n\n`);
      res.end('data: [DONE]\n\n');
      return;
    }
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({
      id: `chatcmpl-${requests.length}`, object: 'chat.completion', model: body.model,
      choices: [{
        index: 0,
        message: { role: 'assistant', ...message },
        finish_reason: message.tool_calls ? 'tool_calls' : 'stop',
      }],
      usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
    }));
  });
});

server.listen(port, '127.0.0.1', () => {
  console.log(`fake-llm em http://127.0.0.1:${server.address().port}/v1`);
});
