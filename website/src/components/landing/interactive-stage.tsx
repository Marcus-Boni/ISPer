"use client";

import { BookOpen, Mic, Pause, Play, RotateCcw, Search, Settings, Sparkles } from "lucide-react";
import { useEffect, useRef, useState } from "react";

const WAVEFORM_BARS = 28;
const BAR_SLOT = 13;
const BAR_WIDTH = 6;
const WAVE_HEIGHT = 72;

/**
 * A speech-shaped profile: fuller through the middle of an utterance, quieter at
 * its edges. Each bar is centred on the midline so the wave reads symmetrically
 * around its axis instead of hanging from a common top edge.
 */
const waveformBars = Array.from({ length: WAVEFORM_BARS }, (_, index) => {
  const position = index / (WAVEFORM_BARS - 1);
  const envelope = 0.52 + 0.48 * Math.sin(Math.PI * position);
  const detail = 0.64 + 0.36 * Math.sin(index * 1.7) * Math.sin(index * 0.9 + 1.3);
  const height = Math.max(16, Math.round(56 * envelope * detail));
  return { x: index * BAR_SLOT, y: (WAVE_HEIGHT - height) / 2, height };
});

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
    const bars = waveform.current.querySelectorAll<SVGRectElement>("rect");
    void import("animejs").then(({ animate, stagger }) => {
      const animation = animate(bars, {
        scaleY: [0.3, 1, 0.45],
        duration: 760,
        delay: stagger(34),
        loop: true,
        alternate: true,
        ease: "inOutSine",
      });
      cancel = () => {
        animation.pause();
        // Hand the bars back to CSS so they ease down to the resting height
        // instead of freezing wherever the loop happened to stop.
        bars.forEach((bar) => bar.style.removeProperty("transform"));
      };
    });
    return () => cancel();
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
            <svg ref={waveform} className={`waveform ${active ? "is-active" : ""}`} viewBox={`0 0 ${WAVEFORM_BARS * BAR_SLOT} ${WAVE_HEIGHT}`} role="img" aria-label={active ? "Forma de onda animada, ditado em andamento" : "Forma de onda em repouso"}>
              {waveformBars.map((bar) => <rect key={bar.x} x={bar.x} y={bar.y} width={BAR_WIDTH} height={bar.height} rx={BAR_WIDTH / 2} />)}
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
