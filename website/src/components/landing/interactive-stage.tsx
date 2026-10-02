"use client";

import { BookOpen, Check, Pause, Play, RotateCcw, Search, Settings, Sparkles } from "lucide-react";
import { Fragment, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { BRAND_MARK } from "@/lib/brand-mark";
import { shortcut, siteConfig } from "@/lib/site";
import { useTablist } from "@/lib/use-tablist";

/**
 * The stage shows the gesture the page is about.
 *
 * Dictation used to be a microphone and a sentence that swapped for another
 * one: the text never went anywhere. The product's whole point is where it
 * goes — you hold the shortcut in whatever app you are using, speak, let go,
 * and the words land in that app. So the dictation scene is another app (a
 * team chat with a question waiting for an answer) and the ISPer indicator
 * floating over it, the way the real one floats over your desktop. The reply
 * streams into the indicator as a provisional caption, and on release it
 * flies out of the indicator and lands in the reply field.
 *
 * It plays by itself once, after the headline has finished being dictated and
 * only while the stage is in view — on a phone that means when the reader
 * scrolls to it. Any interaction cancels that and the reader drives.
 */

const REPLY = ["Consigo", "sim,", "reviso", "depois", "do", "almoço", "e", "te", "aviso."];

/** The headline's own dictation ends here (hero-headline.tsx); the stage waits for it. */
const AFTER_HEADLINE_MS = 3200;

type Phase = "idle" | "listening" | "speaking" | "pasting" | "pasted";

const transcript = [
  { time: "00:04", speaker: "Participante 1", tone: "p1", text: "Vamos fechar as prioridades do lançamento desta semana." },
  { time: "00:10", speaker: "Eu", tone: "me", text: "A página de download precisa explicar CPU e CUDA sem ambiguidade." },
  { time: "00:18", speaker: "Participante 2", tone: "p2", text: "E a documentação deve começar pelo primeiro ditado, não pela arquitetura." },
];

const prefersReducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** The ISPer mark, from the generated geometry — the same five bars and dot as every icon. */
function ListenMark({ speaking }: { speaking: boolean }) {
  return (
    <svg className={`listen-mark${speaking ? " is-speaking" : ""}`} viewBox={BRAND_MARK.viewBox} aria-hidden="true" focusable="false">
      {BRAND_MARK.bars.map((bar, i) => (
        <rect key={bar.x} className={`bar-${i}`} x={bar.x} y={bar.y} width={bar.width} height={bar.height} rx={bar.width / 2} />
      ))}
      <circle className="listen-dot" cx={BRAND_MARK.dot.cx} cy={BRAND_MARK.dot.cy} r={BRAND_MARK.dot.r} />
    </svg>
  );
}

export function InteractiveStage() {
  const [mode, setMode] = useState<"dictation" | "meeting">("dictation");
  const [phase, setPhase] = useState<Phase>("idle");
  const [spoken, setSpoken] = useState(0);
  const [meetingActive, setMeetingActive] = useState(false);
  const [step, setStep] = useState(0);

  const stage = useRef<HTMLDivElement>(null);
  const caption = useRef<HTMLSpanElement>(null);
  const landed = useRef<HTMLSpanElement>(null);
  const timers = useRef<number[]>([]);
  /** Set by any deliberate interaction: from then on the reader drives, not the page. */
  const tookOver = useRef(false);
  const autoplayed = useRef(false);

  const clearTimers = useCallback(() => {
    timers.current.forEach((id) => window.clearTimeout(id));
    timers.current = [];
  }, []);

  const at = useCallback((ms: number, run: () => void) => {
    timers.current.push(window.setTimeout(run, ms));
  }, []);

  /**
   * One dictation, timed like speech: a beat to press the keys and start
   * listening, each word given time in proportion to its length, a beat of
   * silence, then the release.
   */
  const dictate = useCallback(() => {
    clearTimers();
    setPhase("listening");
    setSpoken(0);
    let t = 420;
    at(t, () => setPhase("speaking"));
    REPLY.forEach((word, i) => {
      t += 140 + word.length * 34;
      at(t, () => setSpoken(i + 1));
    });
    t += 420;
    at(t, () => setPhase("pasting"));
    at(t + 760, () => setPhase("pasted"));
  }, [at, clearTimers]);

  const resetDictation = useCallback(() => {
    clearTimers();
    setPhase("idle");
    setSpoken(0);
  }, [clearTimers]);

  /**
   * The flight. The reply is rendered in the field already final; for one
   * moment it is drawn from where the caption sat in the indicator, scaled to
   * the caption's size, and eased into its own place — a shared-element move,
   * so the reader sees the same words travel rather than one disappear and
   * another appear. Without the flight (reduced motion) it crossfades.
   */
  useLayoutEffect(() => {
    if (phase !== "pasting" || !caption.current || !landed.current) return;
    const from = caption.current.getBoundingClientRect();
    const to = landed.current.getBoundingClientRect();
    if (prefersReducedMotion() || !to.width) {
      landed.current.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 220, easing: "ease-out" });
      return;
    }
    const scale = from.height / Math.max(1, to.height);
    landed.current.animate(
      [
        { transform: `translate(${from.left - to.left}px, ${from.top - to.top}px) scale(${scale})`, opacity: 0.55, filter: "blur(0.6px)" },
        { transform: "none", opacity: 1, filter: "none" },
      ],
      { duration: 640, easing: "cubic-bezier(0.16, 1, 0.3, 1)" },
    );
  }, [phase]);

  /**
   * Autoplay, once. It waits for the headline to finish — two dictations
   * running at once would be noise — and for the stage to be on screen, so a
   * phone reader who scrolls down later still gets to watch it happen.
   */
  useEffect(() => {
    const node = stage.current;
    if (!node) return;

    if (prefersReducedMotion()) {
      // No movement, but the outcome stays legible: the reply already sits
      // in the field where the gesture would have put it.
      const id = window.setTimeout(() => { setSpoken(REPLY.length); setPhase("pasted"); }, 0);
      return () => window.clearTimeout(id);
    }

    let pending = 0;
    const observer = new IntersectionObserver(([entry]) => {
      if (!entry?.isIntersecting || autoplayed.current || tookOver.current) return;
      autoplayed.current = true;
      observer.disconnect();
      const wait = Math.max(0, AFTER_HEADLINE_MS - performance.now());
      pending = window.setTimeout(() => { if (!tookOver.current) dictate(); }, wait);
    }, { threshold: 0.45 });
    observer.observe(node);
    return () => { observer.disconnect(); window.clearTimeout(pending); };
  }, [dictate]);

  useEffect(() => clearTimers, [clearTimers]);

  /**
   * The meeting transcript fills one line at a time and then holds. It used to
   * wrap back to zero, which reset the clock too — a recording timer counting
   * down to 00:00 while the chip still read "gravando reunião" is the one
   * detail that tells a reader the state is theatre.
   */
  useEffect(() => {
    if (!meetingActive) return;
    const timer = window.setInterval(() => setStep((value) => Math.min(value + 1, transcript.length)), 1500);
    return () => window.clearInterval(timer);
  }, [meetingActive]);

  const modes = ["dictation", "meeting"] as const;
  const { list: tabList, tabProps } = useTablist(modes, mode, (next) => {
    setMode(next);
    resetDictation();
    setMeetingActive(false);
    setStep(0);
  });

  const takeOver = () => { tookOver.current = true; };

  const recording = mode === "dictation" ? phase === "listening" || phase === "speaking" : meetingActive;
  const busy = phase === "listening" || phase === "speaking" || phase === "pasting";
  const status = mode === "dictation"
    ? (phase === "listening" || phase === "speaking" ? "ouvindo" : phase === "pasting" || phase === "pasted" ? "colado" : "pronto")
    : meetingActive ? "gravando reunião" : "pronto";

  return (
    <div className="app-stage" ref={stage} aria-label="Demonstração interativa do ISPer" onPointerDownCapture={takeOver} onKeyDownCapture={takeOver}>
      <div className="stage-caption"><span>Demonstração</span><span>Conteúdo fictício · nenhum áudio é capturado</span></div>
      <div className="app-window">
        <div className="app-titlebar">
          <div className="app-brand">ISPer<span>.</span><small>{siteConfig.currentVersion}</small></div>
          <div className={`app-state ${recording ? "active" : ""}`} aria-live="polite"><i />{status}</div>
          <div className="app-tools" aria-hidden="true"><span><BookOpen /></span><span><Settings /></span></div>
        </div>
        <div className="stage-tabs" role="tablist" aria-label="Modo da demonstração" ref={tabList}>
          <button id="stage-tab-dictation" type="button" aria-controls="stage-panel" {...tabProps("dictation")}>Ditado</button>
          <button id="stage-tab-meeting" type="button" aria-controls="stage-panel" {...tabProps("meeting")}>Reunião</button>
        </div>
        {mode === "dictation" ? (
          <div id="stage-panel" className={`dictation-scene is-${phase}`} role="tabpanel" aria-labelledby="stage-tab-dictation">
            {/* The other app: the place the words are for. */}
            <div className="target-app">
              <div className="target-head"><span className="target-avatar" aria-hidden="true">EP</span><div><strong>Equipe de produto</strong><small>4 participantes</small></div></div>
              <div className="target-thread">
                <p className="target-bubble"><b>Ana</b>Alguém consegue revisar a página de download hoje?</p>
              </div>
              <div className="target-field" aria-label="Campo de resposta do app de mensagens">
                {phase === "pasting" || phase === "pasted" ? (
                  <span className="target-text" ref={landed}>{REPLY.join(" ")}</span>
                ) : (
                  <span className="target-placeholder">Responder…</span>
                )}
                <i className="target-caret" aria-hidden="true" />
              </div>
            </div>

            {/* The ISPer indicator, floating over whatever app has focus. */}
            <div className="listen-pill" aria-hidden="true">
              <ListenMark speaking={phase === "speaking"} />
              <span className="listen-body">
                {/* The caption stays mounted through the flight: the layout
                    effect measures where it sat to start the words from there.
                    Swapping it for "Colado" in the same step would unmount the
                    very element the flight begins at. */}
                {phase === "idle" ? (
                  <span className="listen-keys">Segure {shortcut.default.map((key, index) => <Fragment key={key}>{index > 0 ? <b>+</b> : null}<kbd>{key}</kbd></Fragment>)}</span>
                ) : phase === "pasted" ? (
                  <span className="listen-done"><Check />Colado no app</span>
                ) : (
                  <span className="listen-caption" ref={caption}>
                    {spoken === 0 ? <span className="listen-hint">ouvindo…</span> : REPLY.slice(0, spoken).join(" ")}
                  </span>
                )}
              </span>
            </div>
          </div>
        ) : (
          <div id="stage-panel" className="meeting-view" role="tabpanel" aria-labelledby="stage-tab-meeting">
            <div className="meeting-toolbar"><span><i className="meeting-dot" />Reunião de lançamento</span><span className="mono">00:{String(Math.min(step * 8, 24)).padStart(2, "0")}</span></div>
            <div className="transcript-list">
              {transcript.map((line, index) => <div className={`transcript-row ${index >= step && meetingActive ? "is-pending" : ""}`} key={line.time}><time>{line.time}</time><p><strong className={line.tone}>{line.speaker}</strong>{line.text}</p></div>)}
            </div>
            <div className="summary-preview"><Sparkles aria-hidden="true" /><div><strong>Resumo opcional</strong><p>Gerado somente com o provedor de IA configurado por você.</p></div></div>
          </div>
        )}
        <div className="stage-controls">
          {mode === "dictation" ? (
            <button type="button" className="stage-primary" onClick={dictate} disabled={busy}>
              <Play aria-hidden="true" />{phase === "pasted" ? "Ditar de novo" : busy ? "Ditando…" : "Experimentar ditado"}
            </button>
          ) : (
            <button type="button" className="stage-primary" onClick={() => setMeetingActive((value) => !value)}>
              {meetingActive ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}{meetingActive ? "Pausar" : "Simular reunião"}
            </button>
          )}
          <button type="button" onClick={() => { if (mode === "dictation") resetDictation(); else { setMeetingActive(false); setStep(0); } }}><RotateCcw aria-hidden="true" />Reiniciar</button>
          <span className="stage-note"><Search aria-hidden="true" />Busca local na Biblioteca</span>
        </div>
      </div>
    </div>
  );
}
