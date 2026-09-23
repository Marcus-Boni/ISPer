// Reunião FICTÍCIA para as capturas da vitrine: injetada no Copilot pelo
// próprio addSegment()/render() da página (o mesmo caminho dos eventos do app),
// então a tela é a real, só os dados é que são inventados. Nomes e falas são
// de exemplo — nunca use aqui trecho de reunião de verdade.
(() => {
  const segs = [
    ['Eu', 12, 18, "Let's lock the scope for phase two today."],
    ['Participante 1', 20, 31, 'Agreed. We ship on the 30th, without the reports module.'],
    ['Participante 2', 41, 52, 'My concern is weekend support. Nobody owns that yet.'],
    ['Eu', 55, 63, 'Carlos, can you send the revised proposal by Friday?'],
    ['Participante 1', 68, 74, "Sure, I'll send it Friday morning."],
  ];
  segs.forEach(([speaker, a, b, text]) => addSegment({ speaker, start_secs: a, end_secs: b, text }));
  render({
    meeting_active: true, configured: true, running: false, error: null,
    last_trigger: null, last_updated: '10:42:15', active_topic: 'Phase two scope and delivery date',
    cards: [
      { id: 'c1', kind: 'decision', title: 'Phase two ships on the 30th', description: 'Without the reports module; it moves to phase three.', urgency: 'high', at_secs: 20, status: 'proposed' },
      { id: 'c2', kind: 'action', title: 'Send the revised proposal', description: '', owner: 'Carlos', due_date: 'Friday', urgency: 'medium', at_secs: 55, status: 'confirmed' },
      { id: 'c3', kind: 'risk', title: 'Weekend support has no owner', description: 'Raised by Participant 2 and still open.', urgency: 'medium', at_secs: 41, status: 'proposed' },
    ],
    memories: [{ id: 'mem-1', meeting_id: 1, title: 'Phase two kickoff', started_at: '09/01/2026 10:00', at_secs: 754, snippet: 'we agreed to revisit the reports module after the launch', score: 0.81 }],
    dynamics_note: null, me_talk_secs: 48, others_talk_secs: 62, monologue: false, elapsed_secs: 1422, scratchpad: '',
  });
  return 'ok';
})()
