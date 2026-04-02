import React, { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import "./capsule.css";

/* ── Constants ─────────────────────────────────────────────────────────── */

const BAR_COUNT = 5;
const BAR_WEIGHTS = [0.5, 0.8, 1.0, 0.75, 0.55];
const ATTACK = 0.4;
const RELEASE = 0.15;
const JITTER = 0.04;
// Width range — must match sync_capsule_window clamp in main.rs
const MIN_W = 260;
const MAX_W = 620;

type Phase = "idle" | "recording" | "processing" | "refining" | "done" | "error";

/* ── Waveform ──────────────────────────────────────────────────────────── */

function Waveform({ active }: { active: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const env = useRef(new Float32Array(BAR_COUNT));
  const rms = useRef(0);
  const raf = useRef(0);
  const on = useRef(active);
  const logged = useRef(false);

  useEffect(() => {
    on.current = active;
    if (!active) {
      rms.current = 0;
      logged.current = false;
      env.current.fill(0);
    }
  }, [active]);

  useEffect(() => {
    const p = listen<number>("rms-level", (e) => {
      if (!on.current) return;
      rms.current = e.payload;
      if (!logged.current && e.payload > 0.01) {
        logged.current = true;
        void invoke("capsule_frontend_log", {
          message: `rms-first:${e.payload.toFixed(4)}`,
        }).catch(() => {});
      }
    });
    return () => { p.then((u) => u()); };
  }, []);

  useEffect(() => {
    const c = canvasRef.current;
    if (!c) return;
    const ctx = c.getContext("2d")!;
    const W = c.width;
    const H = c.height;
    const barW = 4;
    const gap = (W - BAR_COUNT * barW) / (BAR_COUNT + 1);

    const frame = () => {
      ctx.clearRect(0, 0, W, H);
      const input = on.current ? Math.max(0, rms.current) : 0;
      const boosted = Math.min(1, Math.pow(input, 0.32) * 2.6);

      for (let i = 0; i < BAR_COUNT; i++) {
        const targetLevel = boosted
          * BAR_WEIGHTS[i]
          * (1 + (Math.random() * 2 - 1) * JITTER);
        const prev = env.current[i];
        env.current[i] = targetLevel > prev
          ? prev + (targetLevel - prev) * ATTACK
          : prev + (targetLevel - prev) * RELEASE;

        const e = env.current[i];
        const barH = on.current
          ? Math.max(8, 6 + e * (H - 8))
          : Math.max(0, e * H);
        const x = gap + i * (barW + gap);
        const y = (H - barH) / 2;

        const g = ctx.createLinearGradient(x, y, x, y + barH);
        g.addColorStop(0, "rgba(139, 142, 255, 0.95)");
        g.addColorStop(1, "rgba(99, 102, 241, 0.68)");
        ctx.fillStyle = g;

        const r = Math.min(2.5, barW / 2, barH / 2);
        ctx.beginPath();
        ctx.moveTo(x + r, y);
        ctx.lineTo(x + barW - r, y);
        ctx.arcTo(x + barW, y, x + barW, y + r, r);
        ctx.lineTo(x + barW, y + barH - r);
        ctx.arcTo(x + barW, y + barH, x + barW - r, y + barH, r);
        ctx.lineTo(x + r, y + barH);
        ctx.arcTo(x, y + barH, x, y + barH - r, r);
        ctx.lineTo(x, y + r);
        ctx.arcTo(x, y, x + r, y, r);
        ctx.closePath();
        ctx.fill();
      }
      raf.current = requestAnimationFrame(frame);
    };

    raf.current = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf.current);
  }, []);

  return <canvas ref={canvasRef} className="cap-wave" width={44} height={32} />;
}

/* ── Capsule ───────────────────────────────────────────────────────────── */

export default function Capsule() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [transcript, setTranscript] = useState("");
  const [errorMsg, setErrorMsg] = useState("");
  const [show, setShow] = useState(false);
  const [lang, setLang] = useState("zh-CN");
  const phaseRef = useRef<Phase>("idle");
  const hideTimerRef = useRef<number | null>(null);

  const go = (p: Phase) => {
    phaseRef.current = p;
    setPhase(p);
    setShow(p !== "idle");
  };

  const clearHideTimer = () => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
  };

  const resetCapsule = () => {
    clearHideTimer();
    setTranscript("");
    setErrorMsg("");
    go("idle");
  };

  const scheduleReset = (delayMs: number, clearText: boolean) => {
    clearHideTimer();
    hideTimerRef.current = window.setTimeout(() => {
      go("idle");
      if (clearText) {
        setTranscript("");
        setErrorMsg("");
      }
      hideTimerRef.current = null;
    }, delayMs);
  };

  // Language
  useEffect(() => {
    void invoke("capsule_frontend_log", { message: "mounted" }).catch(() => {});
    void invoke("capsule_frontend_log", { message: "initial-phase:idle" }).catch(() => {});
    invoke<{ language: string }>("get_config")
      .then((c) => setLang(c.language === "zh" ? "zh-CN" : c.language || "zh-CN"))
      .catch(() => {});
  }, []);

  // Events
  useEffect(() => {
    const subs: Array<() => void> = [];
    (async () => {
      const init = await invoke<string>("get_capsule_state").catch(() => "idle");
      if (init !== "idle") go(init as Phase);

      subs.push(await listen("recording-started", () => {
        clearHideTimer();
        setTranscript("");
        setErrorMsg("");
        go("recording");
      }));
      subs.push(await listen("processing-started", () => {
        setTranscript("");
        setErrorMsg("");
        go("processing");
      }));
      subs.push(await listen("refining-started", () => go("refining")));
      subs.push(await listen("transcript-clear", () => setTranscript("")));
      subs.push(await listen<string>("transcript-chunk", (e) =>
        setTranscript((p) => p + e.payload)));
      subs.push(await listen<string>("llm-chunk", (e) =>
        setTranscript((p) => p + e.payload)));
      subs.push(await listen("capsule-hide", () => resetCapsule()));
      subs.push(await listen<string>("recording-error", (e) => {
        clearHideTimer();
        setErrorMsg(e.payload);
        go("error");
        scheduleReset(1600, true);
      }));
      subs.push(await listen("recording-done", () => {
        go("done");
        scheduleReset(260, true);
      }));
    })();
    return () => {
      clearHideTimer();
      subs.forEach((u) => u());
    };
  }, []);

  // Polling fallback
  useEffect(() => {
    const id = setInterval(async () => {
      const s = await invoke<string>("get_capsule_state").catch(() => null);
      if (s && s !== phaseRef.current) go(s as Phase);
    }, 120);
    return () => clearInterval(id);
  }, []);

  // ── Derived ────────────────────────────────────────────────────────────

  const text =
    (phase === "recording" ? (transcript || "正在聆听…") : "")
    || (phase === "processing" ? "正在转写…" : "")
    || (phase === "refining" ? "正在润色…" : "")
    || (phase === "error" ? errorMsg : "")
    || transcript;

  // Fixed parts: padL(20) + dot(8) + gap(10) + wave(44) + gapR(14) + pillPadL(0) + pill(~58) + padR(18) ≈ 172px
  // Text adds ~12px per character
  const w = Math.min(MAX_W, Math.max(MIN_W, 172 + Math.max(text.length, 6) * 12));

  // Sync native window size — capsule fills window, so window size = capsule size
  useEffect(() => {
    if (!show) return;
    invoke("sync_capsule_window", { width: w }).catch(() => {});
  }, [show, w]);

  const dim = phase === "processing" || phase === "error";
  const pulse = phase === "refining";
  const textCls = [
    "cap-text",
    dim ? "cap-text--dim" : "",
    pulse ? "cap-text--dim cap-text--blink" : "",
  ].filter(Boolean).join(" ");

  // ── Render ─────────────────────────────────────────────────────────────

  if (!show && phase === "idle") return null;

  return (
    <div className={`capsule ${show ? "capsule--enter" : "capsule--exit"}`}>
      <div className="cap-left">
        <div
          className={`cap-dot${phase === "recording" ? " cap-dot--on" : ""}`}
          style={phase === "error" ? { background: "#ff5d6c" } : undefined}
        />
        <Waveform active={phase === "recording"} />
      </div>
      <div className={textCls}>{text}</div>
      <div className="cap-pill">{lang}</div>
    </div>
  );
}
