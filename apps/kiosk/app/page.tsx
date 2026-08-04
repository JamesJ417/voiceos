"use client";

import { ChangeEvent, FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";

type VoiceState = "ready" | "listening" | "processing" | "speaking" | "error";
type ViewName = "command" | "tasks" | "history" | "system";

type Message = {
  id: string;
  role: "You" | "VIC";
  body: string;
  meta: string;
};

type ConversationFloor = {
  conversation_id: string;
  holder_device_id: string | null;
  holder_display_name: string | null;
  phase: "idle" | "listening" | "processing" | "speaking";
  partial_transcript: string | null;
  response_text: string | null;
  revision: number;
  expires_at_unix: number;
  active: boolean;
};

type CanonicalMessage = {
  sequence: number;
  role: "user" | "assistant" | "system" | "tool";
  content: string;
  provider: string | null;
  origin_device_id: string | null;
  created_at: string;
};

type Approval = { request_id: string; tool: string; expires_at_unix?: number };
type Provider = { name: string; configured?: boolean; role?: string };
type GatewayHealth = {
  status?: string;
  gateway?: string;
  language_model?: string;
  transport?: string;
  speech_to_text?: string;
  text_to_speech?: string;
};
type HostHealth = {
  status?: string;
  disk_free_percent?: number;
  memory_available_percent?: number;
  logical_cpu_count?: number;
};
type SkillProposal = {
  id: string;
  name: string;
  version: number;
  status: string;
  content: string;
  required_capabilities: unknown[];
  evidence: unknown[];
  created_at: string;
  updated_at: string;
};
type SkillUsage = { id: string; skill_id: string; skill_name: string; skill_version: number; outcome: string; feedback: "correct" | "incorrect" | null; used_at: string };
type TaskStep = { id: string; title: string; owner: "user" | "vic" | "shared"; status: string };
type TaskDetail = {
  task: { id: string; title: string; observable_outcome: string; status: string; estimated_minutes: number };
  progress: { completed_steps: number; total_steps: number; open_blockers: number; lane: "needs_me" | "vic_working" | "review" | "shared"; vic_status: string; next_user_action?: string | null; next_vic_action?: string | null };
  steps: TaskStep[];
  blockers: Array<{ id: string; description: string; owner: string; status: string }>;
  artifacts: Array<{ id: string; kind: string; uri: string; description: string }>;
};

type RecognitionResultEvent = {
  resultIndex: number;
  results: ArrayLike<{ isFinal: boolean; 0: { transcript: string } }>;
};

type RecognitionLike = {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  onresult: ((event: RecognitionResultEvent) => void) | null;
  onerror: ((event: { error: string }) => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
  abort(): void;
};

type RecognitionConstructor = new () => RecognitionLike;

const STORAGE = {
  gateway: "voiceos.web.gateway",
  token: "voiceos.web.device-token",
  deviceId: "voiceos.web.device-id",
  session: "voiceos.web.session-id",
  speechRate: "voiceos.web.speech-rate",
  eventCursor: "voiceos.web.event-cursor",
};

const suggestedGateway = "https://voiceos-rig.example.ts.net";

const voiceCopy: Record<VoiceState, { eyebrow: string; title: string; action: string }> = {
  ready: { eyebrow: "Voice channel ready", title: "What can I help with?", action: "Talk" },
  listening: { eyebrow: "Microphone active", title: "I’m listening", action: "Done" },
  processing: { eyebrow: "Private inference", title: "Working on that", action: "Stop" },
  speaking: { eyebrow: "Response playing", title: "VIC is speaking", action: "Interrupt" },
  error: { eyebrow: "Connection needs attention", title: "Let’s reconnect", action: "Retry" },
};

function makeId() {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
}

function cleanGateway(value: string) {
  return value.trim().replace(/\/+$/, "");
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : "VoiceOS could not complete that request.";
}

function isContinueHereCommand(text: string) {
  return /^(vic[,.]?\s*)?(continue|pick up|move|switch)( the conversation)? here[.!?]?$/i.test(text.trim());
}

export default function Home() {
  const [view, setView] = useState<ViewName>("command");
  const [voiceState, setVoiceState] = useState<VoiceState>("ready");
  const [gateway, setGateway] = useState("");
  const [token, setToken] = useState("");
  const [sessionId, setSessionId] = useState("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [health, setHealth] = useState<GatewayHealth>({});
  const [hostHealth, setHostHealth] = useState<HostHealth>({});
  const [connected, setConnected] = useState(false);
  const [statusMessage, setStatusMessage] = useState("Connect this browser to VoiceOS.");
  const [draft, setDraft] = useState("");
  const [liveTranscript, setLiveTranscript] = useState("");
  const [lastResponse, setLastResponse] = useState("");
  const [pendingApproval, setPendingApproval] = useState<Approval | null>(null);
  const [skillProposals, setSkillProposals] = useState<SkillProposal[]>([]);
  const [skills, setSkills] = useState<SkillProposal[]>([]);
  const [skillUsages, setSkillUsages] = useState<SkillUsage[]>([]);
  const [tasks, setTasks] = useState<TaskDetail[]>([]);
  const [taskFilter, setTaskFilter] = useState<"all" | "needs_me" | "vic_working" | "review">("all");
  const [showSettings, setShowSettings] = useState(false);
  const [enrollmentCode, setEnrollmentCode] = useState("");
  const [speechRate, setSpeechRate] = useState(1.25);
  const [floor, setFloor] = useState<ConversationFloor | null>(null);
  const recognition = useRef<RecognitionLike | null>(null);
  const fileInput = useRef<HTMLInputElement | null>(null);

  const request = useCallback(async <T,>(path: string, init: RequestInit = {}): Promise<T> => {
    if (!gateway) throw new Error("Enter the VoiceOS gateway URL in Connection settings.");
    const headers = new Headers(init.headers);
    headers.set("Accept", "application/json");
    if (token) headers.set("Authorization", `Bearer ${token}`);
    if (init.body && !headers.has("Content-Type")) headers.set("Content-Type", "application/json");
    const response = await fetch(`${gateway}${path}`, { ...init, headers, cache: "no-store" });
    const payload = await response.json().catch(() => ({}));
    if (!response.ok) {
      const reason = typeof payload.error === "string" ? payload.error.replaceAll("_", " ") : `HTTP ${response.status}`;
      throw new Error(`VoiceOS gateway: ${reason}`);
    }
    return payload as T;
  }, [gateway, token]);

  const loadHistory = useCallback(async () => {
    const payload = await request<{ conversation_id: string | null; messages: CanonicalMessage[] }>("/v1/conversations/active");
    const history = payload.messages
      .filter((message) => message.role === "user" || message.role === "assistant")
      .map((message) => ({
        id: String(message.sequence),
        role: message.role === "user" ? "You" as const : "VIC" as const,
        body: message.content,
        meta: message.provider ? `${message.provider} · ${formatTime(message.created_at)}` : formatTime(message.created_at),
      }));
    setMessages(history);
    const latest = history.filter((message) => message.role === "VIC").at(-1);
    if (latest) setLastResponse(latest.body);
  }, [request]);

  const loadFloor = useCallback(async () => {
    const payload = await request<{ floor: ConversationFloor | null }>("/v1/conversations/active/floor");
    setFloor(payload.floor);
  }, [request]);

  const changeFloor = useCallback(async (
    action: "claim" | "update" | "release",
    phase: ConversationFloor["phase"] = "listening",
    partialTranscript?: string,
    responseText?: string,
  ) => {
    const payload = await request<{ floor: ConversationFloor }>("/v1/conversations/active/floor", {
      method: "POST",
      body: JSON.stringify({ action, phase, partial_transcript: partialTranscript || null, response_text: responseText || null, display_name: "VoiceOS touch panel", ttl_seconds: 45 }),
    });
    setFloor(payload.floor);
    return payload.floor;
  }, [request]);

  const loadSkillProposals = useCallback(async () => {
    const payload = await request<{ proposals: SkillProposal[] }>("/v1/skills/proposals?status=proposed&limit=20");
    setSkillProposals(payload.proposals);
  }, [request]);

  const loadSkills = useCallback(async () => {
    const [catalog, history] = await Promise.all([
      request<{ skills: SkillProposal[] }>("/v1/skills?status=approved&limit=200"),
      request<{ usages: SkillUsage[] }>("/v1/skills/usages?limit=30"),
    ]);
    setSkills(catalog.skills);
    setSkillUsages(history.usages);
  }, [request]);

  const loadTasks = useCallback(async () => {
    const payload = await request<{ details?: TaskDetail[] }>("/v1/tasks?limit=100");
    setTasks(payload.details ?? []);
  }, [request]);

  const refreshStatus = useCallback(async () => {
    if (!gateway) return;
    try {
      const gatewayHealth = await request<GatewayHealth>("/v1/health");
      setHealth(gatewayHealth);
      const [providerResult, hostResult] = await Promise.allSettled([
        request<{ providers: Provider[] }>("/v1/providers"),
        request<HostHealth>("/v1/tools/system.health"),
      ]);
      if (providerResult.status === "fulfilled") setProviders(providerResult.value.providers);
      if (hostResult.status === "fulfilled") setHostHealth(hostResult.value);
      await Promise.all([
        loadHistory().catch(() => undefined),
        loadSkillProposals().catch(() => undefined),
        loadSkills().catch(() => undefined),
        loadTasks().catch(() => undefined),
        loadFloor().catch(() => undefined),
      ]);
      setConnected(true);
      setStatusMessage(`Connected · ${gatewayHealth.language_model ?? "provider ready"}`);
      setVoiceState("ready");
    } catch (error) {
      setConnected(false);
      setStatusMessage(errorText(error));
      setVoiceState("error");
    }
  }, [gateway, loadFloor, loadHistory, loadSkillProposals, loadSkills, loadTasks, request]);

  useEffect(() => {
    const restore = window.setTimeout(() => {
      const savedGateway = localStorage.getItem(STORAGE.gateway) ?? "";
      const savedToken = localStorage.getItem(STORAGE.token) ?? "";
      const savedSession = localStorage.getItem(STORAGE.session) ?? makeId();
      const savedRate = Number(localStorage.getItem(STORAGE.speechRate) ?? "1.25");
      setGateway(savedGateway);
      setToken(savedToken);
      setSessionId(savedSession);
      setSpeechRate(Number.isFinite(savedRate) ? Math.min(2, Math.max(1, savedRate)) : 1.25);
      localStorage.setItem(STORAGE.session, savedSession);
      if (!savedGateway) setShowSettings(true);
    }, 0);
    return () => window.clearTimeout(restore);
  }, []);

  useEffect(() => {
    if (!gateway) return;
    const refresh = window.setTimeout(() => void refreshStatus(), 0);
    return () => window.clearTimeout(refresh);
  }, [gateway, token, refreshStatus]);

  useEffect(() => {
    if (!gateway || !token) return;
    const timer = window.setInterval(() => void loadFloor().catch(() => undefined), 15_000);
    return () => window.clearInterval(timer);
  }, [gateway, token, loadFloor]);

  useEffect(() => {
    if (!gateway || !token) return;
    const controller = new AbortController();
    let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    const connect = async () => {
      try {
        const after = Number(localStorage.getItem(STORAGE.eventCursor) ?? "0");
        const response = await fetch(`${gateway}/v1/events?after=${Number.isFinite(after) ? after : 0}`, {
          headers: { Accept: "text/event-stream", Authorization: `Bearer ${token}` },
          cache: "no-store",
          signal: controller.signal,
        });
        if (!response.ok || !response.body) throw new Error(`Event stream HTTP ${response.status}`);
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = "";
        while (!stopped) {
          const { done, value } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          const frames = buffer.split("\n\n");
          buffer = frames.pop() ?? "";
          for (const frame of frames) {
            const data = frame.split("\n").find((line) => line.startsWith("data:"))?.slice(5).trim();
            if (!data) continue;
            const event = JSON.parse(data) as { id: number; type: string; payload: Record<string, unknown> };
            localStorage.setItem(STORAGE.eventCursor, String(event.id));
            if (event.type === "conversation.turn") void loadHistory();
            if (event.type === "conversation.floor.changed") {
              const nextFloor = event.payload.floor as ConversationFloor | undefined;
              if (nextFloor) {
                setFloor(nextFloor);
                const thisDevice = localStorage.getItem(STORAGE.deviceId);
                if (nextFloor.active && nextFloor.holder_device_id !== thisDevice) {
                  recognition.current?.abort();
                  speechSynthesis.cancel();
                  setVoiceState("ready");
                  setLiveTranscript(nextFloor.partial_transcript ?? "");
                  setStatusMessage(`Conversation active on ${nextFloor.holder_display_name ?? "another device"}.`);
                }
              }
            }
            if (event.type === "task.changed" || event.type === "task.progress.updated" || event.type === "daily_plan.proposed") {
              setStatusMessage("Shared plan and task state updated.");
              void loadTasks();
            }
            if (event.type === "task.initiative.updated") {
              const detail = typeof event.payload.response_text === "string" ? event.payload.response_text : "VIC advanced a task.";
              setStatusMessage(detail);
            }
            if (event.type === "approval.proposed") {
              setPendingApproval({
                request_id: String(event.payload.request_id ?? ""),
                tool: String(event.payload.tool ?? ""),
                expires_at_unix: Number(event.payload.expires_at_unix ?? 0),
              });
            }
            if (event.type === "approval.decided") setPendingApproval(null);
            if (event.type === "status.changed") {
              setConnected(true);
              setStatusMessage("VoiceOS status updated · online");
            }
          }
        }
        if (!stopped) reconnectTimer = setTimeout(() => void connect(), 2000);
      } catch (error) {
        if (!stopped && !(error instanceof DOMException && error.name === "AbortError")) {
          setStatusMessage("Live sync reconnecting…");
          reconnectTimer = setTimeout(() => void connect(), 2000);
        }
      }
    };
    void connect();
    return () => {
      stopped = true;
      controller.abort();
      if (reconnectTimer) clearTimeout(reconnectTimer);
    };
  }, [gateway, token, loadHistory, loadTasks]);

  useEffect(() => () => {
    recognition.current?.abort();
    speechSynthesis.cancel();
  }, []);

  async function sendText(text: string) {
    const normalized = text.trim();
    if (!normalized || voiceState === "processing") return;
    speechSynthesis.cancel();
    setDraft("");
    setLiveTranscript(normalized);
    setVoiceState("processing");
    await changeFloor("claim", "processing", normalized).catch(() => undefined);
    setMessages((current) => [...current, { id: makeId(), role: "You", body: normalized, meta: "Now" }]);
    try {
      const result = await request<{
        session_id: string;
        response_text: string;
        provider: string;
        processing_ms: number;
        approvals?: Approval[];
      }>("/v1/turns/text", {
        method: "POST",
        body: JSON.stringify({ session_id: sessionId || makeId(), text: normalized }),
      });
      setSessionId(result.session_id);
      localStorage.setItem(STORAGE.session, result.session_id);
      setLastResponse(result.response_text);
      setMessages((current) => [...current, {
        id: makeId(), role: "VIC", body: result.response_text,
        meta: `${result.provider || "VIC"} · ${formatDuration(result.processing_ms)}`,
      }]);
      setPendingApproval(result.approvals?.[0] ?? null);
      setStatusMessage(result.approvals?.length ? "Approval required before the tool can run." : "Response complete");
      const currentDevice = localStorage.getItem(STORAGE.deviceId);
      const speakingFloor = await changeFloor("update", "speaking", normalized, result.response_text).catch(() => null);
      if (speakingFloor?.holder_device_id === currentDevice) speak(result.response_text);
    } catch (error) {
      setStatusMessage(errorText(error));
      setVoiceState("error");
    }
  }

  function speak(text: string) {
    if (!("speechSynthesis" in window) || !text) {
      setVoiceState("ready");
      return;
    }
    speechSynthesis.cancel();
    const utterance = new SpeechSynthesisUtterance(text);
    utterance.rate = speechRate;
    utterance.onstart = () => setVoiceState("speaking");
    utterance.onend = () => { setVoiceState("ready"); void changeFloor("release", "idle").catch(() => undefined); };
    utterance.onerror = () => { setVoiceState("ready"); void changeFloor("release", "idle").catch(() => undefined); };
    speechSynthesis.speak(utterance);
  }

  function startListening() {
    const browserWindow = window as typeof window & {
      SpeechRecognition?: RecognitionConstructor;
      webkitSpeechRecognition?: RecognitionConstructor;
    };
    const Recognition = browserWindow.SpeechRecognition ?? browserWindow.webkitSpeechRecognition;
    if (!Recognition) {
      setStatusMessage("This browser does not provide speech recognition. Chrome or Edge is recommended.");
      setVoiceState("error");
      return;
    }
    const instance = new Recognition();
    recognition.current = instance;
    instance.continuous = false;
    instance.interimResults = true;
    instance.lang = "en-US";
    instance.onresult = (event) => {
      let transcript = "";
      let final = false;
      for (let index = event.resultIndex; index < event.results.length; index += 1) {
        transcript += event.results[index][0].transcript;
        final ||= event.results[index].isFinal;
      }
      const nextTranscript = transcript.trim();
      setLiveTranscript(nextTranscript);
      void changeFloor("update", "listening", nextTranscript).catch(() => undefined);
      if (final) {
        instance.stop();
        if (isContinueHereCommand(transcript)) {
          setStatusMessage("Conversation moved to this touch panel.");
          setVoiceState("ready");
          window.setTimeout(startListening, 250);
        } else {
          void sendText(transcript);
        }
      }
    };
    instance.onerror = (event) => {
      setStatusMessage(`Microphone: ${event.error.replaceAll("-", " ")}`);
      setVoiceState("error");
      void changeFloor("release", "idle").catch(() => undefined);
    };
    instance.onend = () => {
      recognition.current = null;
      setVoiceState((current) => current === "listening" ? "ready" : current);
    };
    setLiveTranscript("");
    void changeFloor("claim", "listening").then(() => {
      setVoiceState("listening");
      instance.start();
    }).catch((error) => {
      setStatusMessage(errorText(error));
      setVoiceState("error");
    });
  }

  function handleTalk() {
    if (voiceState === "listening") {
      recognition.current?.stop();
      setVoiceState("ready");
      void changeFloor("release", "idle").catch(() => undefined);
    } else if (voiceState === "speaking") {
      speechSynthesis.cancel();
      setVoiceState("ready");
      void changeFloor("release", "idle").catch(() => undefined);
    } else if (voiceState === "processing") {
      setStatusMessage("The current response is already processing.");
    } else {
      startListening();
    }
  }

  async function enroll(event: FormEvent) {
    event.preventDefault();
    try {
      const result = await request<{ device_id: string; device_token: string }>("/v1/enrollment/exchange", {
        method: "POST",
        body: JSON.stringify({ code: enrollmentCode.trim(), device_name: `VoiceOS Web · ${navigator.platform || "browser"}` }),
      });
      setToken(result.device_token);
      localStorage.setItem(STORAGE.token, result.device_token);
      localStorage.setItem(STORAGE.deviceId, result.device_id);
      setEnrollmentCode("");
      setStatusMessage("This browser is enrolled.");
      setShowSettings(false);
    } catch (error) {
      setStatusMessage(errorText(error));
    }
  }

  function saveGateway(value: string) {
    const cleaned = cleanGateway(value);
    setGateway(cleaned);
    localStorage.setItem(STORAGE.gateway, cleaned);
    setStatusMessage("Checking the private VoiceOS link…");
  }

  async function decideApproval(approve: boolean) {
    if (!pendingApproval) return;
    if (approve && pendingApproval.tool === "rig.root_command") {
      setStatusMessage("Administrative approval must be confirmed on the enrolled Pixel.");
      return;
    }
    setVoiceState("processing");
    try {
      const result = await request<{ response_text: string; status: string }>("/v1/approvals/decide", {
        method: "POST",
        body: JSON.stringify({ request_id: pendingApproval.request_id, decision: approve ? "approve" : "deny" }),
      });
      setPendingApproval(null);
      setLastResponse(result.response_text);
      setMessages((current) => [...current, { id: makeId(), role: "VIC", body: result.response_text, meta: `Approval · ${result.status}` }]);
      speak(result.response_text);
    } catch (error) {
      setStatusMessage(errorText(error));
      setVoiceState("error");
    }
  }

  async function decideSkillProposal(proposal: SkillProposal, approve: boolean) {
    setStatusMessage(`${approve ? "Approving" : "Rejecting"} ${proposal.name}…`);
    try {
      const result = await request<{ proposal: SkillProposal }>(`/v1/skills/proposals/${encodeURIComponent(proposal.id)}/decision`, {
        method: "POST",
        body: JSON.stringify({ decision: approve ? "approve" : "reject" }),
      });
      setSkillProposals((current) => current.filter((candidate) => candidate.id !== proposal.id));
      setStatusMessage(`${result.proposal.name} was ${result.proposal.status}.`);
      await loadSkills();
    } catch (error) {
      setStatusMessage(errorText(error));
    }
  }

  async function setSkillEnabled(skill: SkillProposal, enabled: boolean) {
    try {
      await request(`/v1/skills/${encodeURIComponent(skill.id)}/status`, { method: "POST", body: JSON.stringify({ status: enabled ? "approved" : "disabled" }) });
      setStatusMessage(`${skill.name} was ${enabled ? "enabled" : "disabled"}.`);
      await loadSkills();
    } catch (error) { setStatusMessage(errorText(error)); }
  }

  async function reviewSkillUsage(usage: SkillUsage, correct: boolean) {
    try {
      await request(`/v1/skills/usages/${encodeURIComponent(usage.id)}/feedback`, { method: "POST", body: JSON.stringify({ feedback: correct ? "correct" : "incorrect" }) });
      setStatusMessage(`${usage.skill_name} use was marked ${correct ? "correct" : "incorrect"}.`);
      await loadSkills();
    } catch (error) { setStatusMessage(errorText(error)); }
  }

  async function uploadFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (file.size > 5 * 1024 * 1024) {
      setStatusMessage("Files are limited to 5 MB.");
      return;
    }
    try {
      const headers = new Headers({
        "Content-Type": file.type || "text/plain",
        "X-VoiceOS-File-Name": encodeURIComponent(file.name),
        "X-VoiceOS-Document-Mode": "reference",
      });
      if (token) headers.set("Authorization", `Bearer ${token}`);
      const response = await fetch(`${gateway}/v1/files`, { method: "POST", headers, body: file });
      const payload = await response.json().catch(() => ({}));
      if (!response.ok) throw new Error(payload.error ?? `Upload failed with HTTP ${response.status}`);
      setStatusMessage(`${file.name} is now available to VoiceOS memory.`);
    } catch (error) {
      setStatusMessage(errorText(error));
    }
  }

  function cycleSpeechRate() {
    const rates = [1, 1.25, 1.5, 1.75, 2];
    const next = rates[(rates.findIndex((rate) => rate === speechRate) + 1) % rates.length];
    setSpeechRate(next);
    localStorage.setItem(STORAGE.speechRate, String(next));
    setStatusMessage(`Speech playback set to ${next}×.`);
  }

  const copy = voiceCopy[voiceState];
  const recentMessages = useMemo(() => messages.slice(-4), [messages]);
  const thisDeviceId = typeof window === "undefined" ? null : localStorage.getItem(STORAGE.deviceId);
  const floorIsRemote = Boolean(floor?.active && floor.holder_device_id && floor.holder_device_id !== thisDeviceId);

  return (
    <main className="shell">
      <aside className="rail" aria-label="VoiceOS navigation">
        <div className="brand"><span className="brand-mark" aria-hidden="true"><i /></span><span>VoiceOS</span></div>
        <nav className="nav-list">
          <NavButton active={view === "command"} label="Command" icon="⌂" onClick={() => setView("command")} />
          <NavButton active={view === "tasks"} label="Tasks" icon="✓" onClick={() => { setView("tasks"); void loadTasks(); }} />
          <NavButton active={view === "history"} label="History" icon="◷" onClick={() => { setView("history"); void loadHistory(); }} />
          <NavButton active={view === "system"} label="System" icon="⌁" onClick={() => { setView("system"); void refreshStatus(); }} />
        </nav>
        <div className="rail-status"><span className={`status-dot ${connected ? "" : "offline"}`} /><div><strong>{connected ? "Private link" : "Disconnected"}</strong><small>{connected ? "VoiceOS connected" : "Open settings"}</small></div></div>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div><p className="kicker">Carbon Command · Web</p><h1>{view === "command" ? "Command center" : view === "tasks" ? "Shared task operations" : view === "history" ? "Conversation history" : "System status"}</h1></div>
          <div className="top-actions">
            <span className={`online-pill ${connected ? "" : "offline-pill"}`}><span className={`status-dot ${connected ? "" : "offline"}`} /> {connected ? "Online" : "Offline"}</span>
            <button className="icon-button" aria-label="Open connection settings" onClick={() => setShowSettings(true)}>⚙</button>
          </div>
        </header>

        <p className={`status-banner ${voiceState === "error" ? "error" : ""}`} role="status">{statusMessage}</p>

        {view === "command" && (
          <div className="dashboard-grid">
            <section className={`voice-panel panel state-${voiceState}`}>
              <div className="voice-copy"><p className="kicker">{copy.eyebrow}</p><h2>{copy.title}</h2><p>{liveTranscript || "Talk naturally. This browser joins the same continuing VoiceOS conversation as your phone."}</p></div>
              <div className="voice-stage">
                <div className="signal-ring ring-one" /><div className="signal-ring ring-two" />
                <button className="talk-hex" onClick={handleTalk} aria-label={`${copy.action}. ${copy.title}`}><span className="mic" aria-hidden="true"><i /></span><strong>{copy.action}</strong><small>{voiceState === "ready" ? "Touch to begin" : copy.eyebrow}</small></button>
              </div>
              <div className="state-track" aria-label={`Current state: ${voiceState}`}>{(["ready", "listening", "processing", "speaking"] as VoiceState[]).map((state) => <span key={state} className={state === voiceState ? "active" : ""}>{state}</span>)}</div>
              {floorIsRemote && <button className="continue-here" onClick={startListening}>Continue here from {floor?.holder_display_name ?? "the other device"}</button>}
              <form className="text-composer" onSubmit={(event) => { event.preventDefault(); void sendText(draft); }}>
                <label htmlFor="voiceos-text">Type a request</label>
                <div><input id="voiceos-text" value={draft} onChange={(event) => setDraft(event.target.value)} placeholder="Ask VIC…" /><button disabled={!draft.trim() || voiceState === "processing"}>Send</button></div>
              </form>
            </section>

            <section className="conversation-panel panel">
              <div className="panel-heading"><div><p className="kicker">Continuous conversation</p><h2>Current thread</h2></div><span className="memory-pill">Memory active</span></div>
              <div className="conversation-list">{recentMessages.length ? recentMessages.map((message) => <MessageCard key={message.id} message={message} />) : <EmptyState text="Your phone and web conversations will appear here." />}</div>
              {pendingApproval && <div className="approval-card"><div><p className="kicker">Approval required</p><strong>{pendingApproval.tool}</strong><small>{pendingApproval.tool === "rig.root_command" ? "Administrative approval is restricted to the enrolled Pixel." : "VIC will not run this tool without your decision."}</small></div><button className="deny" onClick={() => void decideApproval(false)}>Deny</button><button className="approve" disabled={pendingApproval.tool === "rig.root_command"} onClick={() => void decideApproval(true)}>{pendingApproval.tool === "rig.root_command" ? "Approve on Pixel" : "Approve"}</button></div>}
              <button className="skill-review-callout" onClick={() => setView("system")}><span><strong>Skill proposals</strong><small>{skillProposals.length ? `${skillProposals.length} waiting for evidence review` : "No proposals waiting. VoiceOS never enables a generated skill silently."}</small></span><span aria-hidden="true">Review →</span></button>
              <div className="conversation-actions">
                <button disabled={!lastResponse} onClick={() => speak(lastResponse)}>↻ <span>Repeat</span></button>
                <button disabled={!lastResponse} onClick={() => { void navigator.clipboard.writeText(lastResponse); setStatusMessage("Response copied."); }}>▣ <span>Copy reply</span></button>
                <button onClick={() => setView("history")}>◷ <span>Full history</span></button>
                <button onClick={() => fileInput.current?.click()}>＋ <span>Add file</span></button>
                <button onClick={cycleSpeechRate}>{speechRate}× <span>Voice speed</span></button>
              </div>
              <input ref={fileInput} className="visually-hidden" type="file" accept=".txt,.md,.json,.csv,text/plain,text/markdown,application/json,text/csv" onChange={(event) => void uploadFile(event)} />
            </section>

            <ProviderPanel providers={providers} active={health.language_model} />
            <HealthPanel gatewayHealth={health} hostHealth={hostHealth} connected={connected} />
          </div>
        )}

        {view === "history" && <section className="wide-panel panel"><div className="panel-heading"><div><p className="kicker">Persistent timeline</p><h2>Shared conversation with VIC</h2></div><button className="secondary-button" onClick={() => void loadHistory()}>Refresh</button></div><div className="history-list">{messages.length ? messages.map((message) => <MessageCard key={message.id} message={message} />) : <EmptyState text="No conversation history is available yet." />}</div></section>}

        {view === "tasks" && <TaskBoard tasks={tasks} filter={taskFilter} onFilter={setTaskFilter} onRefresh={() => void loadTasks()} />}

        {view === "system" && <div className="system-layout"><SkillProposalPanel proposals={skillProposals} onDecision={(proposal, approve) => void decideSkillProposal(proposal, approve)} onRefresh={() => { void loadSkillProposals(); void loadSkills(); }} /><SkillCatalogPanel skills={skills} usages={skillUsages} onDisable={(skill) => void setSkillEnabled(skill, false)} onFeedback={(usage, correct) => void reviewSkillUsage(usage, correct)} /><ProviderPanel providers={providers} active={health.language_model} wide /><HealthPanel gatewayHealth={health} hostHealth={hostHealth} connected={connected} wide /><section className="panel system-note"><p className="kicker">Privacy boundary</p><h2>Private by default</h2><p>The browser stores only its VoiceOS device credential and preferences. Conversation memory, provider routing, approvals, documents, and audit history remain inside VoiceOS.</p><button className="secondary-button" onClick={() => setShowSettings(true)}>Connection settings</button></section></div>}
      </section>

      {showSettings && <SettingsDialog gateway={gateway} enrollmentCode={enrollmentCode} setEnrollmentCode={setEnrollmentCode} onSaveGateway={saveGateway} onEnroll={enroll} onClose={() => setShowSettings(false)} hasToken={Boolean(token)} onForget={() => { localStorage.removeItem(STORAGE.token); localStorage.removeItem(STORAGE.deviceId); setToken(""); setConnected(false); setStatusMessage("Browser enrollment removed."); }} />}
    </main>
  );
}

function SettingsDialog({ gateway, enrollmentCode, setEnrollmentCode, onSaveGateway, onEnroll, onClose, hasToken, onForget }: { gateway: string; enrollmentCode: string; setEnrollmentCode: (value: string) => void; onSaveGateway: (value: string) => void; onEnroll: (event: FormEvent) => void; onClose: () => void; hasToken: boolean; onForget: () => void }) {
  const [gatewayDraft, setGatewayDraft] = useState(gateway || suggestedGateway);
  return <div className="dialog-backdrop" role="presentation"><section className="settings-dialog panel" role="dialog" aria-modal="true" aria-labelledby="settings-title"><div className="panel-heading"><div><p className="kicker">Private connection</p><h2 id="settings-title">Connect this browser</h2></div><button className="icon-button" aria-label="Close settings" onClick={onClose}>×</button></div><label>VoiceOS gateway URL<input type="url" value={gatewayDraft} onChange={(event) => setGatewayDraft(event.target.value)} placeholder={suggestedGateway} /></label><button className="primary-button" onClick={() => onSaveGateway(gatewayDraft)}>Save and test connection</button><div className="dialog-divider" /><form onSubmit={onEnroll}><label>One-time enrollment code<input inputMode="numeric" autoComplete="one-time-code" value={enrollmentCode} onChange={(event) => setEnrollmentCode(event.target.value)} placeholder="Enter the code from VoiceOS" /></label><button className="primary-button" disabled={!gatewayDraft.trim() || !enrollmentCode.trim()}>{hasToken ? "Replace browser credential" : "Enroll browser"}</button></form>{hasToken && <button className="danger-button" onClick={onForget}>Forget this browser</button>}<p className="dialog-help">The gateway must use HTTPS when this page is opened from a secure URL. Add this site’s exact origin to the gateway’s allowed web origins.</p></section></div>;
}

function NavButton({ active, icon, label, onClick }: { active: boolean; icon: string; label: string; onClick: () => void }) {
  return <button className={active ? "active" : ""} onClick={onClick}><span aria-hidden="true">{icon}</span>{label}</button>;
}

function MessageCard({ message }: { message: Message }) {
  return <article className={`message ${message.role === "You" ? "user" : "assistant"}`}><div className="message-label"><strong>{message.role}</strong><span>{message.meta}</span></div><p>{message.body}</p></article>;
}

function EmptyState({ text }: { text: string }) {
  return <div className="empty-state"><span aria-hidden="true">⬡</span><p>{text}</p></div>;
}

function TaskBoard({ tasks, filter, onFilter, onRefresh }: { tasks: TaskDetail[]; filter: "all" | "needs_me" | "vic_working" | "review"; onFilter: (value: "all" | "needs_me" | "vic_working" | "review") => void; onRefresh: () => void }) {
  const visible = tasks.filter((task) => filter === "all" || task.progress.lane === filter);
  const count = (lane: TaskDetail["progress"]["lane"]) => tasks.filter((task) => task.progress.lane === lane).length;
  return <section className="task-board panel"><div className="panel-heading"><div><p className="kicker">Human + agent execution</p><h2>Task responsibility board</h2></div><button className="secondary-button" onClick={onRefresh}>Refresh</button></div><div className="task-rollups"><button className={filter === "needs_me" ? "active" : ""} onClick={() => onFilter("needs_me")}><span>Needs me</span><strong>{count("needs_me")}</strong></button><button className={filter === "vic_working" ? "active" : ""} onClick={() => onFilter("vic_working")}><span>VIC working</span><strong>{count("vic_working")}</strong></button><button className={filter === "review" ? "active" : ""} onClick={() => onFilter("review")}><span>Ready for review</span><strong>{count("review")}</strong></button><button className={filter === "all" ? "active" : ""} onClick={() => onFilter("all")}><span>All open</span><strong>{tasks.length}</strong></button></div><div className="task-grid">{visible.length ? visible.map((detail) => <article className={`task-card lane-${detail.progress.lane}`} key={detail.task.id}><div className="task-card-head"><span>{detail.progress.lane.replaceAll("_", " ")}</span><strong>{detail.progress.total_steps ? `${detail.progress.completed_steps}/${detail.progress.total_steps} steps` : "No steps"}</strong></div><h3>{detail.task.title}</h3><p>{detail.task.observable_outcome}</p><div className="task-handoff"><small>{detail.progress.lane === "vic_working" ? "VIC NEXT ACTION" : detail.progress.lane === "review" ? "READY FOR REVIEW" : "YOUR NEXT ACTION"}</small><strong>{detail.progress.lane === "vic_working" ? detail.progress.next_vic_action || "Continue safe work" : detail.progress.next_user_action || "Review with VIC"}</strong></div><div className="task-steps">{detail.steps.slice(0, 5).map((step) => <div key={step.id}><span>{step.status === "completed" ? "✓" : "○"}</span><p>{step.title}</p><small>{step.owner}</small></div>)}</div><footer><span>VIC {detail.progress.vic_status.replaceAll("_", " ")}</span><span>{detail.progress.open_blockers} blockers</span><span>{detail.artifacts.length} artifacts</span></footer></article>) : <EmptyState text="No tasks are in this responsibility lane." />}</div></section>;
}

function SkillProposalPanel({ proposals, onDecision, onRefresh }: { proposals: SkillProposal[]; onDecision: (proposal: SkillProposal, approve: boolean) => void; onRefresh: () => void }) {
  return <section className="skill-proposals-panel panel"><div className="panel-heading"><div><p className="kicker">Reviewed self-improvement</p><h2>Skill proposals</h2></div><button className="secondary-button" onClick={onRefresh}>Refresh</button></div><p className="proposal-intro">{proposals.length ? `${proposals.length} evidence-backed proposal${proposals.length === 1 ? "" : "s"} waiting for your decision.` : "Nothing is waiting for review. VoiceOS never enables a generated skill silently."}</p><div className="skill-proposal-list">{proposals.map((proposal) => <article className="skill-proposal-card" key={proposal.id}><div className="skill-proposal-title"><div><span className="proposal-version">Version {proposal.version}</span><h3>{proposal.name}</h3></div><span className="proposal-status">Review required</span></div><div className="proposal-facts"><span><strong>{proposal.evidence.length}</strong> successful audit turns</span><span><strong>{proposal.required_capabilities.length}</strong> typed capabilities</span></div><div className="capability-list">{proposal.required_capabilities.map((capability, index) => <code key={`${String(capability)}-${index}`}>{String(capability)}</code>)}</div><details><summary>Inspect proposed procedure</summary><pre>{proposal.content}</pre></details><details><summary>Inspect source evidence</summary><pre>{JSON.stringify(proposal.evidence, null, 2)}</pre></details><div className="proposal-actions"><button className="deny" onClick={() => onDecision(proposal, false)}>Reject</button><button className="approve" onClick={() => onDecision(proposal, true)}>Approve version</button></div><p className="proposal-safety">Approval records this version for later permissioned use. The proposal itself cannot execute.</p></article>)}</div></section>;
}

function SkillCatalogPanel({ skills, usages, onDisable, onFeedback }: { skills: SkillProposal[]; usages: SkillUsage[]; onDisable: (skill: SkillProposal) => void; onFeedback: (usage: SkillUsage, correct: boolean) => void }) {
  return <section className="skill-proposals-panel panel"><div className="panel-heading"><div><p className="kicker">Active capability library</p><h2>VIC skills</h2></div><span className="healthy-label">{skills.length} active</span></div><div className="skill-proposal-list">{skills.map((skill) => <article className="skill-proposal-card" key={skill.id}><div className="skill-proposal-title"><div><span className="proposal-version">Version {skill.version}</span><h3>{skill.name}</h3></div><span className="provider-state green">Active</span></div><div className="capability-list">{skill.required_capabilities.length ? skill.required_capabilities.map((capability, index) => <code key={`${String(capability)}-${index}`}>{String(capability)}</code>) : <code>coordination procedure</code>}</div><details><summary>Inspect procedure</summary><pre>{skill.content}</pre></details><button className="deny" onClick={() => onDisable(skill)}>Disable skill</button></article>)}</div><div className="panel-heading"><div><p className="kicker">Learning from real use</p><h3>Recent skill activity</h3></div></div><div className="skill-proposal-list">{usages.length ? usages.slice(0, 10).map((usage) => <article className="skill-proposal-card" key={usage.id}><div className="skill-proposal-title"><div><span className="proposal-version">Version {usage.skill_version}</span><h3>{usage.skill_name}</h3></div><span className="proposal-status">{usage.outcome}</span></div>{usage.feedback ? <p className="proposal-safety">Reviewed: {usage.feedback}</p> : <div className="proposal-actions"><button className="approve" onClick={() => onFeedback(usage, true)}>Used correctly</button><button className="deny" onClick={() => onFeedback(usage, false)}>Used incorrectly</button></div>}</article>) : <EmptyState text="VIC has not used an approved typed workflow since tracking was enabled." />}</div></section>;
}

function ProviderPanel({ providers, active, wide = false }: { providers: Provider[]; active?: string; wide?: boolean }) {
  const displayProviders = providers.length ? providers : [
    { name: "ollama", role: "Fast local voice" },
    { name: "ollama-deep", role: "Deep local reasoning" },
    { name: "codex-sol", role: "Highest confidence" },
  ];
  return <section className={`provider-panel panel ${wide ? "wide" : ""}`}><div className="panel-heading"><div><p className="kicker">Reasoning fabric</p><h2>Model providers</h2></div></div><div className="provider-list">{displayProviders.slice(0, 5).map((provider) => { const selected = provider.name === active; return <div className="provider-row" key={provider.name}><span className={`provider-glyph ${selected ? "green" : "cyan"}`} aria-hidden="true">{selected ? "✦" : "◎"}</span><div><strong>{providerLabel(provider.name)}</strong><small>{provider.role ?? "VoiceOS provider"}</small></div><span className={`provider-state ${selected ? "green" : provider.configured === false ? "amber" : "cyan"}`}>{selected ? "Active" : provider.configured === false ? "Offline" : "Ready"}</span></div>; })}</div></section>;
}

function HealthPanel({ gatewayHealth, hostHealth, connected, wide = false }: { gatewayHealth: GatewayHealth; hostHealth: HostHealth; connected: boolean; wide?: boolean }) {
  return <section className={`health-panel panel ${wide ? "wide" : ""}`}><div className="panel-heading"><div><p className="kicker">Live infrastructure</p><h2>System health</h2></div><span className={connected ? "healthy-label" : "online-pill"}>{connected ? "Connected" : "Unavailable"}</span></div><div className="metric-grid"><Metric label="Gateway" value={gatewayHealth.gateway ?? "—"} /><Metric label="Host" value={hostHealth.status ?? "—"} /><Metric label="Disk free" value={percent(hostHealth.disk_free_percent)} /><Metric label="Memory free" value={percent(hostHealth.memory_available_percent)} /><Metric label="Logical CPUs" value={hostHealth.logical_cpu_count?.toString() ?? "—"} /><Metric label="Transport" value={gatewayHealth.transport ?? "private"} /></div></section>;
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div className="metric"><span>{label}</span><strong>{value}</strong></div>;
}

function providerLabel(name: string) {
  if (name === "ollama") return "Gemma";
  if (name === "ollama-deep") return "gpt-oss";
  if (name === "codex-sol") return "Codex Sol";
  return name;
}

function formatDuration(milliseconds?: number) {
  if (!milliseconds) return "complete";
  return milliseconds < 1000 ? `${milliseconds} ms` : `${(milliseconds / 1000).toFixed(1)} sec`;
}

function formatTime(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

function percent(value?: number) {
  return typeof value === "number" ? `${value.toFixed(1)}%` : "—";
}
