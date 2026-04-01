import React, { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// ── Types ─────────────────────────────────────────────────────────────────────

interface SttConfig {
  base_url: string;
  api_key: string;
  model: string;
}

type SttBackend = "windows_speech" | "custom";

interface LlmConfig {
  enabled: boolean;
  base_url: string;
  api_key: string;
  model: string;
}

interface AppConfig {
  language: string;
  auto_start: boolean;
  stt_backend: SttBackend;
  stt: SttConfig;
  llm: LlmConfig;
}

type TestStatus = "idle" | "testing" | "ok" | "error";

// ── Helpers ───────────────────────────────────────────────────────────────────

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section style={{ marginBottom: 28 }}>
      <h2 style={{
        fontSize: 11,
        fontWeight: 600,
        letterSpacing: "0.1em",
        textTransform: "uppercase",
        color: "rgba(160,160,200,0.7)",
        marginBottom: 12,
      }}>
        {title}
      </h2>
      <div style={{
        background: "rgba(255,255,255,0.04)",
        borderRadius: 12,
        border: "1px solid rgba(255,255,255,0.08)",
        overflow: "hidden",
      }}>
        {children}
      </div>
    </section>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      padding: "12px 16px",
      borderBottom: "1px solid rgba(255,255,255,0.06)",
      gap: 12,
    }}>
      <label style={{
        width: 110,
        fontSize: 13,
        color: "rgba(220,220,240,0.85)",
        flexShrink: 0,
      }}>
        {label}
      </label>
      {children}
    </div>
  );
}

function TextInput({
  value,
  onChange,
  type = "text",
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  type?: string;
  placeholder?: string;
}) {
  return (
    <input
      type={type}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      style={{
        flex: 1,
        background: "rgba(255,255,255,0.06)",
        border: "1px solid rgba(255,255,255,0.1)",
        borderRadius: 6,
        padding: "6px 10px",
        color: "#e8e8f0",
        fontSize: 13,
        fontFamily: "inherit",
        outline: "none",
        transition: "border-color 0.15s",
      }}
      onFocus={(e) => (e.currentTarget.style.borderColor = "rgba(100,140,255,0.6)")}
      onBlur={(e) => (e.currentTarget.style.borderColor = "rgba(255,255,255,0.1)")}
    />
  );
}

function TestButton({
  status,
  onClick,
}: {
  status: TestStatus;
  onClick: () => void;
}) {
  const label =
    status === "testing" ? "Testing…"
    : status === "ok" ? "✓ OK"
    : status === "error" ? "✗ Failed"
    : "Test";

  const color =
    status === "ok" ? "#44cc88"
    : status === "error" ? "#ff6666"
    : "rgba(255,255,255,0.7)";

  return (
    <button
      onClick={onClick}
      disabled={status === "testing"}
      style={{
        padding: "5px 12px",
        background: "rgba(255,255,255,0.08)",
        border: "1px solid rgba(255,255,255,0.12)",
        borderRadius: 6,
        color,
        fontSize: 12,
        cursor: status === "testing" ? "default" : "pointer",
        transition: "all 0.15s",
        fontFamily: "inherit",
        flexShrink: 0,
      }}
    >
      {label}
    </button>
  );
}

// ── Main Settings Component ───────────────────────────────────────────────────

export default function Settings() {
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [sttStatus, setSttStatus] = useState<TestStatus>("idle");
  const [llmStatus, setLlmStatus] = useState<TestStatus>("idle");
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [sttMsg, setSttMsg] = useState("");
  const [llmMsg, setLlmMsg] = useState("");
  const sttTestGeneration = useRef(0);

  useEffect(() => {
    invoke<AppConfig>("get_config").then(setCfg);
  }, []);

  if (!cfg) {
    return (
      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100%" }}>
        <span style={{ color: "rgba(200,200,220,0.5)", fontSize: 14 }}>Loading…</span>
      </div>
    );
  }

  const updateStt = (patch: Partial<SttConfig>) =>
    setCfg((c) => c && { ...c, stt: { ...c.stt, ...patch } });

  const updateLlm = (patch: Partial<LlmConfig>) =>
    setCfg((c) => c && { ...c, llm: { ...c.llm, ...patch } });

  const resetSttTestFeedback = () => {
    sttTestGeneration.current += 1;
    setSttStatus("idle");
    setSttMsg("");
  };

  const handleSave = async () => {
    setSaveStatus("saving");
    try {
      await invoke("save_config", { newCfg: cfg });
      setSaveStatus("saved");
      setTimeout(() => setSaveStatus("idle"), 2000);
    } catch {
      setSaveStatus("error");
      setTimeout(() => setSaveStatus("idle"), 3000);
    }
  };

  const handleTestStt = async () => {
    const generation = sttTestGeneration.current + 1;
    sttTestGeneration.current = generation;
    setSttStatus("testing");
    setSttMsg("");
    try {
      const msg = await invoke<string>("test_stt", { cfg });
      if (sttTestGeneration.current !== generation) {
        return;
      }
      setSttMsg(msg);
      setSttStatus("ok");
    } catch (e) {
      if (sttTestGeneration.current !== generation) {
        return;
      }
      setSttMsg(String(e));
      setSttStatus("error");
    }
    setTimeout(() => {
      if (sttTestGeneration.current === generation) {
        setSttStatus("idle");
      }
    }, 5000);
  };

  const handleTestLlm = async () => {
    setLlmStatus("testing");
    setLlmMsg("");
    try {
      const msg = await invoke<string>("test_llm", { cfg: cfg.llm });
      setLlmMsg(msg);
      setLlmStatus("ok");
    } catch (e) {
      setLlmMsg(String(e));
      setLlmStatus("error");
    }
    setTimeout(() => setLlmStatus("idle"), 5000);
  };

  return (
    <div style={{
      padding: "24px 28px",
      maxWidth: 560,
      margin: "0 auto",
      height: "100%",
      overflow: "auto",
    }}>
      {/* Header */}
      <div style={{ marginBottom: 28 }}>
        <h1 style={{ fontSize: 20, fontWeight: 700, color: "#e8e8f8", marginBottom: 4 }}>
          VoiceInput Settings
        </h1>
        <p style={{ fontSize: 12, color: "rgba(160,160,190,0.7)" }}>
          Hold <kbd style={{ background: "rgba(255,255,255,0.1)", padding: "1px 6px", borderRadius: 4, fontSize: 11 }}>Right Alt</kbd> to record
        </p>
      </div>

      {/* STT Configuration */}
      <Section title="Speech-to-Text (STT)">
        <Field label="Backend">
          <select
            value={cfg.stt_backend}
            onChange={(e) => {
              resetSttTestFeedback();
              setCfg((c) => c && { ...c, stt_backend: e.target.value as SttBackend });
            }}
            style={{
              flex: 1,
              background: "rgba(255,255,255,0.06)",
              border: "1px solid rgba(255,255,255,0.1)",
              borderRadius: 6,
              padding: "6px 10px",
              color: "#e8e8f0",
              fontSize: 13,
              fontFamily: "inherit",
              outline: "none",
              cursor: "pointer",
            }}
          >
            <option value="windows_speech">Windows SpeechRecognizer</option>
            <option value="custom">Custom STT</option>
          </select>
          <TestButton status={sttStatus} onClick={handleTestStt} />
        </Field>
        {cfg.stt_backend === "windows_speech" ? (
          <div style={{
            padding: "12px 16px",
            fontSize: 12,
            color: "rgba(220,220,240,0.75)",
            borderBottom: "1px solid rgba(255,255,255,0.06)",
            lineHeight: 1.5,
          }}>
            Recognition language is managed by Windows system speech settings.
          </div>
        ) : (
          <>
        <Field label="API Base URL">
          <TextInput
            value={cfg.stt.base_url}
            onChange={(v) => updateStt({ base_url: v })}
            placeholder="https://api.openai.com"
          />
        </Field>
        <Field label="API Key">
          <TextInput
            type="password"
            value={cfg.stt.api_key}
            onChange={(v) => updateStt({ api_key: v })}
            placeholder="sk-…"
          />
        </Field>
        <Field label="Model">
          <TextInput
            value={cfg.stt.model}
            onChange={(v) => updateStt({ model: v })}
            placeholder="whisper-1"
          />
        </Field>
          </>
        )}
        {sttMsg && (
          <div style={{
            padding: "8px 16px",
            fontSize: 12,
            color: sttStatus === "ok" ? "#44cc88" : "#ff8888",
          }}>
            {sttMsg}
          </div>
        )}
      </Section>

      {/* LLM Configuration */}
      <Section title="LLM Refinement">
        <Field label="Enabled">
          <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
            <input
              type="checkbox"
              checked={cfg.llm.enabled}
              onChange={(e) => updateLlm({ enabled: e.target.checked })}
              style={{ width: 16, height: 16, accentColor: "#6677ff" }}
            />
            <span style={{ fontSize: 13, color: "rgba(220,220,240,0.7)" }}>
              Auto-correct ASR errors with LLM
            </span>
          </label>
        </Field>
        <Field label="API Base URL">
          <TextInput
            value={cfg.llm.base_url}
            onChange={(v) => updateLlm({ base_url: v })}
            placeholder="https://api.openai.com"
          />
        </Field>
        <Field label="API Key">
          <TextInput
            type="password"
            value={cfg.llm.api_key}
            onChange={(v) => updateLlm({ api_key: v })}
            placeholder="sk-…"
          />
        </Field>
        <Field label="Model">
          <TextInput
            value={cfg.llm.model}
            onChange={(v) => updateLlm({ model: v })}
            placeholder="gpt-4o-mini"
          />
          <TestButton status={llmStatus} onClick={handleTestLlm} />
        </Field>
        {llmMsg && (
          <div style={{
            padding: "8px 16px",
            fontSize: 12,
            color: llmStatus === "ok" ? "#44cc88" : "#ff8888",
          }}>
            {llmMsg}
          </div>
        )}
      </Section>

      {/* Language */}
      <Section title="Language">
        <Field label="Default">
          <select
            value={cfg.language}
            onChange={(e) => setCfg((c) => c && { ...c, language: e.target.value })}
            style={{
              flex: 1,
              background: "rgba(255,255,255,0.06)",
              border: "1px solid rgba(255,255,255,0.1)",
              borderRadius: 6,
              padding: "6px 10px",
              color: "#e8e8f0",
              fontSize: 13,
              fontFamily: "inherit",
              outline: "none",
              cursor: "pointer",
            }}
          >
            <option value="zh">简体中文 (zh)</option>
            <option value="zh-TW">繁體中文 (zh-TW)</option>
            <option value="en">English (en)</option>
            <option value="ja">日本語 (ja)</option>
            <option value="ko">한국어 (ko)</option>
          </select>
        </Field>
        <div style={{
          padding: "0 16px 12px",
          fontSize: 12,
          color: "rgba(220,220,240,0.65)",
          lineHeight: 1.5,
        }}>
          This language setting is only used by Custom STT.
        </div>
      </Section>

      {/* Save button */}
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 12 }}>
        <button
          onClick={handleSave}
          disabled={saveStatus === "saving"}
          style={{
            padding: "9px 28px",
            background: saveStatus === "saved"
              ? "rgba(68,200,136,0.25)"
              : saveStatus === "error"
              ? "rgba(255,80,80,0.25)"
              : "rgba(100,120,255,0.35)",
            border: "1px solid",
            borderColor: saveStatus === "saved"
              ? "rgba(68,200,136,0.5)"
              : saveStatus === "error"
              ? "rgba(255,80,80,0.5)"
              : "rgba(100,120,255,0.5)",
            borderRadius: 8,
            color: saveStatus === "saved" ? "#44cc88" : saveStatus === "error" ? "#ff8888" : "#aabbff",
            fontSize: 14,
            fontWeight: 600,
            cursor: saveStatus === "saving" ? "default" : "pointer",
            transition: "all 0.2s",
            fontFamily: "inherit",
          }}
        >
          {saveStatus === "saving" ? "Saving…"
            : saveStatus === "saved" ? "✓ Saved"
            : saveStatus === "error" ? "Save Failed"
            : "Save"}
        </button>
      </div>

      {/* Footer note */}
      <p style={{
        marginTop: 24,
        fontSize: 11,
        color: "rgba(140,140,170,0.5)",
        textAlign: "center",
        lineHeight: 1.6,
      }}>
        API keys are encrypted with Windows DPAPI before being stored.
        <br />
        Config path: %APPDATA%\VoiceInput\config.json
      </p>
    </div>
  );
}
