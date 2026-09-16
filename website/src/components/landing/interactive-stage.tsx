"use client";

import { BookOpen, Mic, Pause, Play, RotateCcw, Search, Settings, Sparkles } from "lucide-react";
import { useEffect, useRef, useState } from "react";

const transcript = [
  { time: "00:04", speaker: "Participante 1", tone: "p1", text: "Vamos fechar as prioridades do lançamento desta semana." },
  { time: "00:10", speaker: "Eu", tone: "me", text: "A página de download precisa explicar CPU e CUDA sem ambiguidade." },
  { time: "00:18", speaker: "Participante 2", tone: "p2", text: "E a documentação deve começar pelo primeiro ditado, não pela arquitetura." },
];

export function InteractiveStage() {
  const [mode, setMode] = useState<"dictation" | "meeting">("dictation");
  const [active, setActive] = useState(false);
  const [step, setStep] = useState(0);
  const waveform = useRef<SVGSVGElement>(null);

  useEffect(() => {
    if (!active || !waveform.current || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    let cancel = () => {};
    void import("animejs").then(({ animate, stagger }) => {
      const animation = animate(waveform.current?.querySelectorAll("rect") ?? [], {
        scaleY: [0.35, 1, 0.5],
        duration: 760,
        delay: stagger(34),
        loop: true,
        alternate: true,
        ease: "inOutSine",
      });
      cancel = () => animation.pause();
    });
    return cancel;
  }, [active]);

  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setStep((value) => (value + 1) % (transcript.length + 1)), 1500);
    return () => window.clearInterval(timer);
  }, [active]);

  const reset = () => { setActive(false); setStep(0); };

  return (
    <div className="app-stage" aria-label="Demonstração interativa do ISPer">
      <div className="stage-caption"><span>Demonstração</span><span>Conteúdo fictício · nenhum áudio é capturado</span></div>
      <div className="app-window">
        <div className="app-titlebar">
          <div className="app-brand">ISPer<span>.</span><small>v0.15.0</small></div>
          <div className={`app-state ${active ? "active" : ""}`}><i />{active ? (mode === "dictation" ? "ouvindo" : "gravando reunião") : "pronto"}</div>
          <div className="app-tools"><button type="button" aria-label="Biblioteca"><BookOpen /></button><button type="button" aria-label="Configurações"><Settings /></button></div>
        </div>
        <div className="stage-tabs" role="tablist" aria-label="Modo da demonstração">
          <button id="stage-tab-dictation" type="button" role="tab" aria-selected={mode === "dictation"} aria-controls="stage-panel-dictation" onClick={() => { setMode("dictation"); reset(); }}>Ditado</button>
          <button id="stage-tab-meeting" type="button" role="tab" aria-selected={mode === "meeting"} aria-controls="stage-panel-meeting" onClick={() => { setMode("meeting"); reset(); }}>Reunião</button>
        </div>
        {mode === "dictation" ? (
          <div id="stage-panel-dictation" className="dictation-view" role="tabpanel" aria-labelledby="stage-tab-dictation">
            <div className="shortcut-line"><span>Atalho configurável</span><div><kbd>Ctrl</kbd><b>+</b><kbd>Shift</kbd><b>+</b><kbd>Espaço</kbd></div></div>
            <div className={`dictation-orb ${active ? "active" : ""}`}><Mic aria-hidden="true" /></div>
            <svg ref={waveform} className="waveform" viewBox="0 0 360 72" role="img" aria-label={active ? "Forma de onda animada, ditado em andamento" : "Forma de onda parada"}>
              {Array.from({ length: 28 }, (_, index) => <rect key={index} x={index * 13} y={18} width="6" height={16 + ((index * 7) % 34)} rx="3" style={{ transformOrigin: `${index * 13 + 3}px 36px` }} />)}
            </svg>
            <p className="dictation-copy">{active ? "A documentação precisa ser clara desde o primeiro clique." : "Segure para falar. Solte para colar no aplicativo em foco."}</p>
          </div>
        ) : (
          <div id="stage-panel-meeting" className="meeting-view" role="tabpanel" aria-labelledby="stage-tab-meeting">
            <div className="meeting-toolbar"><span><i className="meeting-dot" />Reunião de lançamento</span><span className="mono">00:{String(step * 8).padStart(2, "0")}</span></div>
            <div className="transcript-list">
              {transcript.map((line, index) => <div className={`transcript-row ${index >= step && active ? "is-pending" : ""}`} key={line.time}><time>{line.time}</time><p><strong className={line.tone}>{line.speaker}</strong>{line.text}</p></div>)}
            </div>
            <div className="summary-preview"><Sparkles aria-hidden="true" /><div><strong>Resumo opcional</strong><p>Gerado somente com o provedor de IA configurado por você.</p></div></div>
          </div>
        )}
        <div className="stage-controls">
          <button type="button" className="stage-primary" onClick={() => setActive((value) => !value)}>{active ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}{active ? "Pausar" : mode === "dictation" ? "Experimentar ditado" : "Simular reunião"}</button>
          <button type="button" onClick={reset}><RotateCcw aria-hidden="true" />Reiniciar</button>
          <span><Search aria-hidden="true" />Busca local na Biblioteca</span>
        </div>
      </div>
    </div>
  );
}
