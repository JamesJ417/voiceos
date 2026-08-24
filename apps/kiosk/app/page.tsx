"use client";

import { ChangeEvent, FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { approvalDecisionFromText } from "./approval-intent.js";
import {
  ComponentRegistry,
  DEFAULT_VOICEOS_GATEWAY,
  FocusSnapshot,
  VoiceOSClient,
  cleanGateway,
  VOICEOS_ENDPOINTS,
} from "./lib/voiceos-client";

type VoiceState = "ready" | "listening" | "processing" | "speaking" | "error";
type ViewName = "command" | "focus" | "projects" | "tasks" | "memory" | "history" | "system";

type Message = {
  id: string;
  role: "You" | "VIC";
  body: string;
  meta: string;
  images?: Array<{ filename: string; url: string }>;
};
type PendingImage = { id: string; filename: string };
type VicMemory = { id: string; content: string; source: string; category: string; status: string; confidence: number; provenance: string; created_at: string; updated_at: string };
type SleepCycleChange = { id: string; detail: string; status: string; confidence: number | null; created_at: string };
type SleepCycleReport = { cycle: { id: string; mode: string; created_at: string }; changes: SleepCycleChange[] };

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
  attachments?: Array<{ id: string; filename: string; media_type: string }>;
};

type Approval = { request_id: string; tool: string; expires_at_unix?: number };
type AgentWorker = { id: string; status: string; label: string; detail?: string };
type AgentActivity = { id: string; phase: string; label: string; detail?: string; taskId?: string };
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
type VicProject = { id: string; goal_id: string | null; title: string; status: string; created_at: string; updated_at: string };
type TaskStep = { id: string; title: string; owner: "user" | "vic" | "shared"; status: string };
type TaskDetail = {
  task: { id: string; project_id: string | null; title: string; observable_outcome: string; status: string; estimated_minutes: number; due_at: string | null; importance: "low" | "normal" | "high" | "critical" };
  progress: { completed_steps: number; total_steps: number; open_blockers: number; lane: "needs_me" | "vic_working" | "review" | "shared"; vic_status: string; next_user_action?: string | null; next_vic_action?: string | null };
  steps: TaskStep[];
  blockers: Array<{ id: string; description: string; owner: string; status: string }>;
  artifacts: Array<{ id: string; kind: string; uri: string; description: string }>;
  activity: Array<{ id?: string; event_type: string; payload: Record<string, unknown>; occurred_at: string }>;
};

const STORAGE = {
  gateway: "voiceos.web.gateway",
  token: "voiceos.web.device-token",
  deviceId: "voiceos.web.device-id",
  session: "voiceos.web.session-id",
  speechRate: "voiceos.web.speech-rate",
  eventCursor: "voiceos.web.event-cursor",
};

const suggestedGateway = DEFAULT_VOICEOS_GATEWAY;

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

function errorText(error: unknown) {
  return error instanceof Error ? error.message : "Touch could not complete that request.";
}

function isContinueHereCommand(text: string) {
  return /^(vic[,.]?\s*)?(continue|pick up|move|switch)( the conversation)? here[.!?]?$/i.test(text.trim());
}

function currentDeviceId() {
  return localStorage.getItem(STORAGE.deviceId) || "development-device";
}

export default function Home() {
  useEffect(() => {
    if ("serviceWorker" in navigator) {
      void navigator.serviceWorker.register("/sw.js", { scope: "/" });
    }
  }, []);

  const [view, setView] = useState<ViewName>("command");
  const [voiceState, setVoiceState] = useState<VoiceState>("ready");
  const [gateway, setGateway] = useState("");
  const [token, setToken] = useState("");
  const [sessionId, setSessionId] = useState("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [memories, setMemories] = useState<VicMemory[]>([]);
  const [sleepCycles, setSleepCycles] = useState<SleepCycleReport[]>([]);
  const [memoryReviewBusy, setMemoryReviewBusy] = useState(false);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [health, setHealth] = useState<GatewayHealth>({});
  const [hostHealth, setHostHealth] = useState<HostHealth>({});
  const [componentRegistry, setComponentRegistry] = useState<ComponentRegistry | null>(null);
  const [connected, setConnected] = useState(false);
  const [statusMessage, setStatusMessage] = useState("Connect Touch to the VoiceOS backend.");
  const [draft, setDraft] = useState("");
  const [pendingImage, setPendingImage] = useState<PendingImage | null>(null);
  const [liveTranscript, setLiveTranscript] = useState("");
  const [lastResponse, setLastResponse] = useState("");
  const [pendingApproval, setPendingApproval] = useState<Approval | null>(null);
  const [agentWorkers, setAgentWorkers] = useState<AgentWorker[]>([]);
  const [agentActivity, setAgentActivity] = useState<AgentActivity[]>([]);
  const [skillProposals, setSkillProposals] = useState<SkillProposal[]>([]);
  const [skills, setSkills] = useState<SkillProposal[]>([]);
  const [skillUsages, setSkillUsages] = useState<SkillUsage[]>([]);
  const [projects, setProjects] = useState<VicProject[]>([]);
  const [tasks, setTasks] = useState<TaskDetail[]>([]);
  const [focus, setFocus] = useState<FocusSnapshot | null>(null);
  const [focusBusy, setFocusBusy] = useState(false);
  const [taskFilter, setTaskFilter] = useState<"all" | "needs_me" | "vic_working" | "review">("all");
  const [showSettings, setShowSettings] = useState(false);
  const [overlayExpanded, setOverlayExpanded] = useState(false);
  const [enrollmentCode, setEnrollmentCode] = useState("");
  const [speechRate, setSpeechRate] = useState(1.25);
  const [floor, setFloor] = useState<ConversationFloor | null>(null);
  const recorder = useRef<MediaRecorder | null>(null);
  const microphoneStream = useRef<MediaStream | null>(null);
  const audioPlayback = useRef<HTMLAudioElement | null>(null);
  const speechGeneration = useRef(0);
  const audioChunks = useRef<Blob[]>([]);
  const fileInput = useRef<HTMLInputElement | null>(null);
  const imageInput = useRef<HTMLInputElement | null>(null);

  const client = useMemo(() => new VoiceOSClient(gateway, token), [gateway, token]);

  const request = useCallback(async <T,>(path: string, init: RequestInit = {}): Promise<T> => {
    return client.request<T>(path, init);
  }, [client]);

  const loadHistory = useCallback(async () => {
    const payload = await request<{ conversation_id: string | null; messages: CanonicalMessage[] }>("/v1/conversations/active");
    const history = await Promise.all(payload.messages
      .filter((message) => message.role === "user" || message.role === "assistant")
      .map(async (message) => ({
        id: String(message.sequence),
        role: message.role === "user" ? "You" as const : "VIC" as const,
        body: message.content,
        meta: message.provider ? `${message.provider} · ${formatTime(message.created_at)}` : formatTime(message.created_at),
        images: (await Promise.all((message.attachments ?? []).map(async (attachment) => {
          const response = await client.fetch(`/v1/attachments/${attachment.id}`);
          if (!response.ok) return null;
          return { filename: attachment.filename, url: URL.createObjectURL(await response.blob()) };
        }))).filter((image): image is { filename: string; url: string } => image !== null),
      })));
    setMessages(history);
    const latest = history.filter((message) => message.role === "VIC").at(-1);
    if (latest) setLastResponse(latest.body);
  }, [client, request]);

  const loadMemories = useCallback(async (query = "") => {
    const payload = await request<{ memories: VicMemory[] }>(`/v1/memories?limit=200&query=${encodeURIComponent(query)}`);
    setMemories(payload.memories);
  }, [request]);

  const loadMemoryReview = useCallback(async () => {
    const payload = await request<{ sleep_cycles: SleepCycleReport[] }>("/v1/memory/sleep-cycles?limit=14");
    setSleepCycles(payload.sleep_cycles);
  }, [request]);

  async function scanMemoryProposals() {
    setMemoryReviewBusy(true);
    try {
      await request("/v1/memory/sleep-cycles", { method: "POST", body: JSON.stringify({ idempotency_key: `vic-panel-scan-${Date.now()}` }) });
      await loadMemoryReview();
      setStatusMessage("VIC finished the memory scan. Review each proposal before saving it.");
    } finally {
      setMemoryReviewBusy(false);
    }
  }

  async function approveMemoryProposal(cycleId: string, changeId: string) {
    setMemoryReviewBusy(true);
    try {
      await request(`/v1/memory/sleep-cycles/${encodeURIComponent(cycleId)}/commit`, {
        method: "POST",
        body: JSON.stringify({ idempotency_key: `vic-panel-commit-${cycleId}-${changeId}`, change_ids: [changeId] }),
      });
      await Promise.all([loadMemoryReview(), loadMemories()]);
      setStatusMessage("VIC added the approved proposal to durable memory.");
    } finally {
      setMemoryReviewBusy(false);
    }
  }

  async function addMemory(content: string, category: string) {
    await request("/v1/memories", { method: "POST", body: JSON.stringify({ content, category }) });
    await loadMemories();
    setStatusMessage("VIC saved that as durable memory.");
  }

  async function correctMemory(memory: VicMemory) {
    const content = window.prompt("Replace this memory with the corrected fact:", memory.content)?.trim();
    if (!content || content === memory.content) return;
    await request(`/v1/memories/${encodeURIComponent(memory.id)}/correct`, { method: "POST", body: JSON.stringify({ content, category: memory.category }) });
    await loadMemories();
    setStatusMessage("VIC corrected the memory and retained its provenance.");
  }

  async function forgetMemory(memory: VicMemory) {
    if (!window.confirm(`Tell VIC to forget: “${memory.content}”?`)) return;
    await request(`/v1/memories/${encodeURIComponent(memory.id)}`, { method: "DELETE" });
    await loadMemories();
    setStatusMessage("VIC forgot that memory.");
  }

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
      body: JSON.stringify({ action, phase, partial_transcript: partialTranscript || null, response_text: responseText || null, display_name: "Touch panel", ttl_seconds: 45 }),
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

  const loadProjects = useCallback(async () => {
    const payload = await request<{ projects: VicProject[] }>("/v1/projects?limit=100");
    setProjects(payload.projects);
  }, [request]);

  const loadFocus = useCallback(async (mode: FocusSnapshot["mode"] = "normal") => {
    setFocus(await client.focus(mode));
  }, [client]);

  const startFocus = useCallback(async (minutes: 5 | 20, taskId?: string) => {
    setFocusBusy(true);
    try {
      const mode = minutes === 5 ? "five_minute" : "normal";
      setFocus(await client.startFocus({ task_id: taskId, mode, planned_minutes: minutes }));
      setStatusMessage(`${minutes}-minute focus session started. Only this now.`);
    } catch (error) {
      setStatusMessage(errorText(error));
    } finally {
      setFocusBusy(false);
    }
  }, [client]);

  const actFocus = useCallback(async (
    sessionId: string,
    action: "interrupt" | "resume" | "complete",
    nextAction?: string,
  ) => {
    setFocusBusy(true);
    try {
      setFocus(await client.actFocus(sessionId, {
        action,
        ...(action === "interrupt" ? { note: "Interrupted from Touch", restart_action: nextAction } : {}),
        ...(action === "resume" ? { planned_minutes: 5 } : {}),
        ...(action === "complete" ? { reflection: "Ended from Touch" } : {}),
      }));
      setStatusMessage(action === "interrupt" ? "Your place is saved. No guilt, no lost context." : action === "resume" ? "Welcome back. Start with the saved next action." : "Focus session saved. The task stays open until it is actually done.");
    } catch (error) {
      setStatusMessage(errorText(error));
    } finally {
      setFocusBusy(false);
    }
  }, [client]);

  const switchFocus = useCallback(async (taskId: string) => {
    setFocusBusy(true);
    try {
      setFocus(await client.switchFocus(taskId, 5));
      setStatusMessage("Focus switched deliberately. VIC saved the old restart point.");
    } catch (error) {
      setStatusMessage(errorText(error));
    } finally {
      setFocusBusy(false);
    }
  }, [client]);

  const captureFocus = useCallback(async (input: { title: string; due_at?: string; importance?: "low" | "normal" | "high" | "critical" }) => {
    setFocusBusy(true);
    try {
      setFocus(await client.captureFocus(input));
      await Promise.all([loadProjects(), loadTasks()]);
      setStatusMessage("Idea parked. Your current focus did not change.");
    } catch (error) {
      setStatusMessage(errorText(error));
      throw error;
    } finally {
      setFocusBusy(false);
    }
  }, [client, loadProjects, loadTasks]);

  const promoteCapture = useCallback(async (taskId: string) => {
    setFocusBusy(true);
    try {
      await request(`/v1/tasks/${encodeURIComponent(taskId)}/status`, { method: "POST", body: JSON.stringify({ status: "ready" }) });
      await Promise.all([loadFocus(), loadTasks()]);
      setStatusMessage("Moved from the parking lot into actionable work.");
    } catch (error) {
      setStatusMessage(errorText(error));
    } finally {
      setFocusBusy(false);
    }
  }, [loadFocus, loadTasks, request]);

  const refreshStatus = useCallback(async () => {
    if (!gateway) return;
    try {
      const gatewayHealth = await request<GatewayHealth>(VOICEOS_ENDPOINTS.health);
      setHealth(gatewayHealth);
      const [bootstrapResult, providerResult, hostResult] = await Promise.allSettled([
        client.bootstrap(),
        request<{ providers: Provider[] }>(VOICEOS_ENDPOINTS.providers),
        request<HostHealth>(VOICEOS_ENDPOINTS.systemHealth),
      ]);
      if (bootstrapResult.status === "fulfilled") setComponentRegistry(bootstrapResult.value.component_registry);
      if (providerResult.status === "fulfilled") setProviders(providerResult.value.providers);
      if (hostResult.status === "fulfilled") setHostHealth(hostResult.value);
      await Promise.all([
        loadHistory().catch(() => undefined),
        loadSkillProposals().catch(() => undefined),
        loadSkills().catch(() => undefined),
        loadProjects().catch(() => undefined),
        loadTasks().catch(() => undefined),
        loadFocus().catch(() => undefined),
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
  }, [client, gateway, loadFloor, loadFocus, loadHistory, loadProjects, loadSkillProposals, loadSkills, loadTasks, request]);

  useEffect(() => {
    const restore = window.setTimeout(() => {
      const savedGateway = localStorage.getItem(STORAGE.gateway) ?? suggestedGateway;
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
    if (!gateway || connected) return;
    const reconnect = window.setInterval(() => void refreshStatus(), 3_000);
    return () => window.clearInterval(reconnect);
  }, [connected, gateway, refreshStatus]);

  useEffect(() => {
    if (!gateway) return;
    const timer = window.setInterval(() => void loadFloor().catch(() => undefined), 15_000);
    return () => window.clearInterval(timer);
  }, [gateway, token, loadFloor]);

  useEffect(() => {
    if (!gateway) return;
    const controller = new AbortController();
    let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    const connect = async () => {
      try {
        const after = Number(localStorage.getItem(STORAGE.eventCursor) ?? "0");
        const response = await client.fetch(`${VOICEOS_ENDPOINTS.events}?after=${Number.isFinite(after) ? after : 0}`, {
          headers: { Accept: "text/event-stream" },
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
                if (nextFloor.active) {
                  setView("command");
                  const thisDevice = currentDeviceId();
                  const externalVoice = nextFloor.holder_display_name === "VIC desktop microphone" || nextFloor.holder_device_id !== thisDevice;
                  if (externalVoice) {
                    recorder.current?.stop();
                    speechSynthesis.cancel();
                  }
                  setVoiceState(
                    nextFloor.phase === "speaking" ? "speaking" :
                    nextFloor.phase === "processing" ? "processing" : "listening"
                  );
                  setLiveTranscript(nextFloor.partial_transcript ?? "");
                  if (nextFloor.response_text) setLastResponse(nextFloor.response_text);
                  setStatusMessage(
                    nextFloor.phase === "speaking" ? "VIC is responding in this window." :
                    nextFloor.phase === "processing" ? "VIC is working on your voice request." :
                    `Listening through ${nextFloor.holder_display_name ?? "Touch"}.`
                  );
                  if (nextFloor.phase === "speaking") void loadHistory();
                } else {
                  setVoiceState("ready");
                  setLiveTranscript("");
                  setStatusMessage("Voice channel ready");
                }
              }
            }
            if (event.type === "task.changed" || event.type === "task.progress.updated" || event.type === "daily_plan.proposed") {
              setStatusMessage("Shared plan and task state updated.");
              void loadTasks();
            }
            if (event.type === "focus.updated") {
              setStatusMessage("VIC saved the latest focus state.");
              void loadFocus();
            }
            if (event.type === "project.changed") {
              setStatusMessage("VIC project list updated.");
              void Promise.all([loadProjects(), loadTasks()]);
            }
            if (event.type === "task.initiative.updated") {
              const detail = typeof event.payload.response_text === "string" ? event.payload.response_text : "VIC advanced a task.";
              setStatusMessage(detail);
              void loadTasks();
            }
            if (event.type === "agent.worker.updated") {
              const worker = {
                id: String(event.payload.worker_id ?? event.id),
                status: String(event.payload.status ?? "running"),
                label: String(event.payload.label ?? "VIC background worker"),
                detail: typeof event.payload.detail === "string" ? event.payload.detail : undefined,
              };
              setAgentWorkers((current) => [worker, ...current.filter((item) => item.id !== worker.id)].slice(0, 8));
              setStatusMessage(worker.status === "running" ? `VIC worker active · ${worker.label}` : `VIC worker ${worker.status} · ${worker.label}`);
            }
            if (event.type === "agent.activity.updated") {
              const activitySession = String(event.payload.session_id ?? "");
              const activity = {
                id: `${event.id}-${String(event.payload.phase ?? "activity")}`,
                phase: String(event.payload.phase ?? "activity"),
                label: String(event.payload.label ?? "VIC is working"),
                detail: typeof event.payload.detail === "string" ? event.payload.detail : undefined,
                taskId: activitySession.startsWith("task:") ? activitySession.slice(5) : undefined,
              };
              setAgentActivity((current) => {
                const prior = activity.phase === "response.drafting"
                  ? current.filter((item) => item.phase !== "response.drafting")
                  : current;
                return [activity, ...prior].slice(0, 8);
              });
              setStatusMessage(activity.detail ? `${activity.label} · ${activity.detail}` : activity.label);
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
              setStatusMessage("Touch status updated · online");
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
  }, [client, gateway, loadFocus, loadHistory, loadProjects, loadTasks]);

  useEffect(() => () => {
    recorder.current?.stop();
    audioPlayback.current?.pause();
    audioPlayback.current = null;
    speechSynthesis.cancel();
  }, []);

  async function sendText(text: string) {
    const normalized = text.trim();
    if (!normalized || voiceState === "processing") return;
    const approvalDecision = pendingApproval ? approvalDecisionFromText(normalized) : null;
    if (approvalDecision) {
      setDraft("");
      setMessages((current) => [...current, { id: makeId(), role: "You", body: normalized, meta: "Now" }]);
      await decideApproval(approvalDecision === "approve");
      return;
    }
    speechSynthesis.cancel();
    const attachment = pendingImage;
    const requestId = makeId();
    setDraft("");
    setAgentActivity([]);
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
        headers: { "Idempotency-Key": requestId },
        body: JSON.stringify({
          session_id: sessionId || makeId(),
          text: normalized,
          attachment_ids: attachment ? [attachment.id] : [],
        }),
      });
      setPendingImage(null);
      setSessionId(result.session_id);
      localStorage.setItem(STORAGE.session, result.session_id);
      setLastResponse(result.response_text);
      setMessages((current) => [...current, {
        id: makeId(), role: "VIC", body: result.response_text,
        meta: `${result.provider || "VIC"} · ${formatDuration(result.processing_ms)}`,
      }]);
      setPendingApproval(result.approvals?.[0] ?? null);
      setStatusMessage(result.approvals?.length ? "Approval required before the tool can run." : "Response complete");
      const currentDevice = currentDeviceId();
      const speakingFloor = await changeFloor("update", "speaking", normalized, result.response_text).catch(() => null);
      if (!speakingFloor || speakingFloor.holder_device_id === currentDevice) speak(result.response_text);
    } catch (error) {
      setStatusMessage(errorText(error));
      setVoiceState("error");
    }
  }

  async function speak(text: string) {
    const generation = ++speechGeneration.current;
    let spokeNeural = false;
    audioPlayback.current?.pause();
    audioPlayback.current = null;
    speechSynthesis.cancel();
    if (gateway && text) {
      try {
        const chunks = text.match(/[^.!?]+[.!?]+|[^.!?]+$/g)?.map((part) => part.trim()).filter(Boolean) ?? [text];
        let nextAudio = fetchNeuralSpeech(chunks[0]);
        setVoiceState("speaking");
        for (let index = 0; index < chunks.length; index += 1) {
          const audioUrl = await nextAudio;
          if (generation !== speechGeneration.current) return;
          nextAudio = index + 1 < chunks.length ? fetchNeuralSpeech(chunks[index + 1]) : Promise.resolve(null);
          await playNeuralAudio(audioUrl);
          spokeNeural = true;
        }
        audioPlayback.current = null;
        setVoiceState("ready");
        void changeFloor("release", "idle").catch(() => undefined);
        return;
      } catch {
        // Keep speech available if the host neural voice is temporarily offline.
      }
    }
    if (spokeNeural) {
      setVoiceState("ready");
      void changeFloor("release", "idle").catch(() => undefined);
      return;
    }
    speakWithBrowserVoice(text);
  }

  async function fetchNeuralSpeech(text: string) {
    const headers = new Headers({ "Content-Type": "application/json" });
    const response = await client.fetch("/v1/speech/synthesize", { method: "POST", headers, body: JSON.stringify({ text }) });
    if (!response.ok) throw new Error(`Speech HTTP ${response.status}`);
    return URL.createObjectURL(await response.blob());
  }

  function playNeuralAudio(url: string | null) {
    if (!url) return Promise.resolve();
    return new Promise<void>((resolve, reject) => {
      const audio = new Audio(url);
      audioPlayback.current = audio;
      audio.playbackRate = speechRate;
      audio.onended = () => { URL.revokeObjectURL(url); resolve(); };
      audio.onpause = () => { URL.revokeObjectURL(url); resolve(); };
      audio.onerror = () => { URL.revokeObjectURL(url); reject(new Error("Neural speech playback failed")); };
      void audio.play().catch(reject);
    });
  }

  function speakWithBrowserVoice(text: string) {
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

  async function startListening() {
    setLiveTranscript("");
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      microphoneStream.current = stream;
      audioChunks.current = [];
      const instance = new MediaRecorder(stream);
      recorder.current = instance;
      instance.ondataavailable = (event) => {
        if (event.data.size) audioChunks.current.push(event.data);
      };
      instance.onstop = () => {
        recorder.current = null;
        microphoneStream.current?.getTracks().forEach((track) => track.stop());
        microphoneStream.current = null;
        const recording = new Blob(audioChunks.current, { type: instance.mimeType || "audio/webm" });
        setVoiceState("processing");
        setStatusMessage("Transcribing microphone audio…");
        void request<{ transcript: string }>("/v1/transcriptions", {
          method: "POST",
          headers: { "Content-Type": recording.type },
          body: recording,
        }).then(({ transcript }) => {
          setLiveTranscript(transcript);
          if (isContinueHereCommand(transcript)) {
            setVoiceState("ready");
            setStatusMessage("Conversation moved to this touch panel.");
          } else {
            void sendText(transcript);
          }
        }).catch((error) => {
          setStatusMessage(errorText(error));
          setVoiceState("error");
        });
      };
      await changeFloor("claim", "listening").catch(() => null);
      setVoiceState("listening");
      instance.start();
      setStatusMessage("Listening through the selected microphone. Press Done when finished.");
    } catch (error) {
      setStatusMessage(errorText(error));
      setVoiceState("error");
    }
  }

  function handleTalk() {
    if (voiceState === "listening") {
      recorder.current?.stop();
    } else if (voiceState === "speaking") {
      speechGeneration.current += 1;
      audioPlayback.current?.pause();
      audioPlayback.current = null;
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
        body: JSON.stringify({ code: enrollmentCode.trim(), device_name: `Touch · ${navigator.platform || "browser"}` }),
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
      const response = await client.fetch("/v1/files", { method: "POST", headers, body: file });
      const payload = await response.json().catch(() => ({}));
      if (!response.ok) throw new Error(payload.error ?? `Upload failed with HTTP ${response.status}`);
      setStatusMessage(`${file.name} is now available to VIC memory.`);
    } catch (error) {
      setStatusMessage(errorText(error));
    }
  }

  async function uploadImage(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (file.size > 5 * 1024 * 1024) {
      setStatusMessage("Images are limited to 5 MB.");
      return;
    }
    try {
      const headers = new Headers({
        "Content-Type": file.type,
        "X-VoiceOS-File-Name": encodeURIComponent(file.name),
      });
      const response = await client.fetch("/v1/attachments", { method: "POST", headers, body: file });
      const payload = await response.json().catch(() => ({})) as { attachment?: PendingImage; error?: string };
      if (!response.ok || !payload.attachment) throw new Error(payload.error ?? `Upload failed with HTTP ${response.status}`);
      setPendingImage(payload.attachment);
      setStatusMessage(`${file.name} is ready to send with your next message.`);
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
  const latestVicMessageId = useMemo(
    () => [...messages].reverse().find((message) => message.role === "VIC")?.id,
    [messages],
  );
  const thisDeviceId = typeof window === "undefined" ? null : localStorage.getItem(STORAGE.deviceId);
  const floorIsRemote = Boolean(floor?.active && floor.holder_device_id && floor.holder_device_id !== thisDeviceId);
  const vicOverlayExpanded = overlayExpanded;

  return (
    <main className="shell">
      <aside className="rail" aria-label="Touch navigation">
        <div className="brand"><span className="brand-mark" aria-hidden="true"><i /></span><span>Touch</span></div>
        <nav className="nav-list">
          <NavButton active={view === "command"} label="Command" icon="⌂" onClick={() => setView("command")} />
          <NavButton active={view === "focus"} label="Focus" icon="◎" onClick={() => { setView("focus"); void loadFocus(); }} />
          <NavButton active={view === "projects"} label="Projects" icon="▦" onClick={() => { setView("projects"); void Promise.all([loadProjects(), loadTasks()]); }} />
          <NavButton active={view === "tasks"} label="Tasks" icon="✓" onClick={() => { setView("tasks"); void loadTasks(); }} />
          <NavButton active={view === "memory"} label="Memory" icon="◈" onClick={() => { setView("memory"); void Promise.all([loadMemories(), loadMemoryReview()]); }} />
          <NavButton active={view === "history"} label="History" icon="◷" onClick={() => { setView("history"); void loadHistory(); }} />
          <NavButton active={view === "system"} label="System" icon="⌁" onClick={() => { setView("system"); void refreshStatus(); }} />
        </nav>
        <div className="rail-status"><span className={`status-dot ${connected ? "" : "offline"}`} /><div><strong>{connected ? "Private link" : "Disconnected"}</strong><small>{connected ? "VIC connected" : "Open settings"}</small></div></div>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div><p className="kicker">Touch · VIC voice</p><h1>{view === "command" ? "Talk with VIC" : view === "focus" ? "Focus with VIC" : view === "projects" ? "Projects with VIC" : view === "tasks" ? "Shared task operations" : view === "memory" ? "What VIC remembers" : view === "history" ? "Conversation history" : "System status"}</h1></div>
          <div className="top-actions">
            <span className={`online-pill ${connected ? "" : "offline-pill"}`}><span className={`status-dot ${connected ? "" : "offline"}`} /> {connected ? "Online" : "Offline"}</span>
            <button className="icon-button" aria-label="Open connection settings" onClick={() => setShowSettings(true)}>⚙</button>
          </div>
        </header>

        <p className={`status-banner ${voiceState === "error" ? "error" : ""}`} role="status">{statusMessage}</p>

        {view === "command" && (
          <div className="dashboard-grid">
            <section className={`voice-panel panel state-${voiceState}`}>
              <div className="voice-copy"><p className="kicker">{copy.eyebrow}</p><h2>{copy.title}</h2><p>{liveTranscript || "Talk naturally. Touch joins the same continuing VIC conversation as your phone."}</p></div>
              <div className="voice-stage">
                <div className="signal-ring ring-one" /><div className="signal-ring ring-two" />
                <button className="talk-hex" onClick={handleTalk} aria-label={`${copy.action}. ${copy.title}`}><span className="mic" aria-hidden="true"><i /></span><strong>{copy.action}</strong><small>{voiceState === "ready" ? "Touch to begin" : copy.eyebrow}</small></button>
              </div>
              <div className="state-track" aria-label={`Current state: ${voiceState}`}>{(["ready", "listening", "processing", "speaking"] as VoiceState[]).map((state) => <span key={state} className={state === voiceState ? "active" : ""}>{state}</span>)}</div>
              {floorIsRemote && <button className="continue-here" onClick={startListening}>Continue here from {floor?.holder_display_name ?? "the other device"}</button>}
              <form className="text-composer" onSubmit={(event) => { event.preventDefault(); void sendText(draft); }}>
                <label htmlFor="voiceos-text">Type a request</label>
                <div><input id="voiceos-text" value={draft} onChange={(event) => setDraft(event.target.value)} placeholder="Ask VIC…" /><button disabled={!draft.trim() || voiceState === "processing"}>Send</button></div>
                <div><button type="button" onClick={() => imageInput.current?.click()}>＋ Image</button>{pendingImage && <span>{pendingImage.filename} ready</span>}</div>
              </form>
              <input ref={imageInput} className="visually-hidden" type="file" accept="image/jpeg,image/png,image/webp" onChange={(event) => void uploadImage(event)} />
            </section>

            <CommandTaskSummary tasks={tasks} onOpenLane={(lane) => { setTaskFilter(lane); setView("tasks"); }} />

            <section className="conversation-panel panel">
              <div className="panel-heading"><div><p className="kicker">Continuous conversation</p><h2>Current thread</h2></div><span className="memory-pill">Memory active</span></div>
              <div className="conversation-list">{recentMessages.length ? recentMessages.map((message) => <MessageCard key={message.id} message={message} latestVic={message.id === latestVicMessageId} />) : <EmptyState text="Your phone and web conversations will appear here." />}</div>
              <div className={`agent-activity ${agentActivity.length ? "has-activity" : "activity-idle"}`} aria-label="VIC live activity"><div className="agent-activity-title"><span className="thinking-pulse" /><strong>VIC activity</strong><small>Live progress, not private chain-of-thought</small></div>{agentActivity.length ? agentActivity.slice(0, 6).map((activity) => { const isTool = activity.phase.startsWith("tool."); const completed = activity.phase.endsWith("completed"); return <div className={`agent-activity-row ${isTool ? "tool-execution" : ""} ${completed ? "activity-complete" : "activity-running"}`} key={activity.id}>{isTool && <span className="tool-scan" aria-hidden="true" />}<span className="activity-glyph">{isTool ? "⌘" : activity.phase.startsWith("subagent.") ? "◇" : "✦"}</span><p><strong>{activity.label}</strong>{activity.detail && <small>{activity.detail}</small>}</p>{isTool && <span className="execution-state">{completed ? "done" : "called"}</span>}</div>; }) : <div className="activity-empty"><span className="activity-glyph">⌘</span><p><strong>{voiceState === "processing" ? "Connecting to Hermes…" : "Execution rail ready"}</strong><small>Tool calls and worker progress will appear here.</small></p><span className="execution-state">{voiceState === "processing" ? "waiting" : "idle"}</span></div>}</div>
              {agentWorkers.length > 0 && <section className="agent-worker-panel" aria-label="VIC background workers"><div className="agent-worker-heading"><span>◇</span><strong>Hermes subagents</strong><small>{agentWorkers.filter((worker) => worker.status === "running" || worker.status === "queued").length} active</small></div><div className="agent-worker-list">{agentWorkers.map((worker) => <div className={`agent-worker worker-${worker.status}`} key={worker.id}><span className={`status-dot ${worker.status === "failed" ? "offline" : ""}`} /><div><strong>{worker.label}</strong><small>{worker.detail ?? "Dispatched by VIC"}</small></div><span className="worker-state">{worker.status}</span></div>)}</div></section>}
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

        {view === "history" && <section className="wide-panel panel"><div className="panel-heading"><div><p className="kicker">Persistent timeline</p><h2>Shared conversation with VIC</h2></div><button className="secondary-button" onClick={() => void loadHistory()}>Refresh</button></div><div className="history-list">{messages.length ? messages.map((message) => <MessageCard key={message.id} message={message} latestVic={message.id === latestVicMessageId} />) : <EmptyState text="No conversation history is available yet." />}</div></section>}

        {view === "memory" && <MemoryPanel memories={memories} sleepCycles={sleepCycles} reviewBusy={memoryReviewBusy} onScan={scanMemoryProposals} onApprove={approveMemoryProposal} onSearch={loadMemories} onAdd={addMemory} onCorrect={correctMemory} onForget={forgetMemory} />}

        {view === "focus" && <FocusPanel focus={focus} busy={focusBusy} onRefresh={() => void loadFocus()} onLowEnergy={() => void loadFocus("low_energy")} onStart={(minutes, taskId) => void startFocus(minutes, taskId)} onSwitch={(taskId) => void switchFocus(taskId)} onCapture={captureFocus} onPromote={(taskId) => void promoteCapture(taskId)} onAction={(sessionId, action, nextAction) => void actFocus(sessionId, action, nextAction)} />}

        {view === "projects" && <ProjectsPanel projects={projects} tasks={tasks} onRefresh={() => void Promise.all([loadProjects(), loadTasks()])} onCreate={async (title) => {
          await request("/v1/projects", { method: "POST", body: JSON.stringify({ title }) });
          await loadProjects();
          setStatusMessage(`${title} is now connected to VIC.`);
        }} onAssign={async (taskId, projectId) => {
          await request(`/v1/tasks/${encodeURIComponent(taskId)}/project`, { method: "POST", body: JSON.stringify({ project_id: projectId }) });
          await loadTasks();
          const project = projects.find((item) => item.id === projectId);
          setStatusMessage(project ? `Task moved into ${project.title}.` : "Task moved back to Loose work.");
        }} />}

        {view === "tasks" && <TaskBoard projects={projects} tasks={tasks} activity={agentActivity} filter={taskFilter} onFilter={setTaskFilter} onRefresh={() => void Promise.all([loadProjects(), loadTasks()])} onAttention={async (taskId, input) => {
          await request(`/v1/tasks/${encodeURIComponent(taskId)}/attention`, { method: "POST", body: JSON.stringify(input) });
          await Promise.all([loadTasks(), loadFocus()]);
          setStatusMessage("VIC updated when this work should rise to the top.");
        }} onStart={async (input) => {
          setStatusMessage(`Starting VIC on ${input.title}…`);
          await request("/v1/tasks", { method: "POST", body: JSON.stringify(input) });
          setTaskFilter("vic_working");
          await loadTasks();
          setStatusMessage(`VIC started working on ${input.title}. Progress will appear on the task.`);
        }} />}

        {view === "system" && <div className="system-layout"><ComponentRegistryPanel registry={componentRegistry} /><SkillProposalPanel proposals={skillProposals} onDecision={(proposal, approve) => void decideSkillProposal(proposal, approve)} onRefresh={() => { void loadSkillProposals(); void loadSkills(); }} /><SkillCatalogPanel skills={skills} usages={skillUsages} onDisable={(skill) => void setSkillEnabled(skill, false)} onFeedback={(usage, correct) => void reviewSkillUsage(usage, correct)} /><ProviderPanel providers={providers} active={health.language_model} wide /><HealthPanel gatewayHealth={health} hostHealth={hostHealth} connected={connected} wide /><section className="panel system-note"><p className="kicker">Privacy boundary</p><h2>Private by default</h2><p>The screen stores only its Touch device credential and preferences. VIC conversation memory, provider routing, approvals, documents, and audit history remain inside the local VoiceOS services.</p><button className="secondary-button" onClick={() => setShowSettings(true)}>Connection settings</button></section></div>}
      </section>

      {showSettings && <SettingsDialog gateway={gateway} enrollmentCode={enrollmentCode} setEnrollmentCode={setEnrollmentCode} onSaveGateway={saveGateway} onEnroll={enroll} onClose={() => setShowSettings(false)} hasToken={Boolean(token)} onForget={() => { localStorage.removeItem(STORAGE.token); localStorage.removeItem(STORAGE.deviceId); setToken(""); setConnected(false); setStatusMessage("Browser enrollment removed."); }} />}

      <aside className={`vic-overlay state-${voiceState} ${vicOverlayExpanded ? "expanded" : "collapsed"}`} aria-label="VIC desktop presence">
        <button className="vic-orb" onClick={() => setOverlayExpanded((current) => !current)} aria-expanded={vicOverlayExpanded} aria-label={vicOverlayExpanded ? "Collapse VIC desktop presence" : "Open VIC desktop presence"}>
          <span className="vic-orb-core" aria-hidden="true"><i /><i /><i /></span>
          <span className="vic-wave" aria-hidden="true"><i /><i /><i /><i /><i /></span>
          <span className="vic-orb-state">{voiceState}</span>
        </button>
        {vicOverlayExpanded && <div className="vic-overlay-card">
          <div className="vic-overlay-heading"><div><p>VIC</p><strong>{copy.title}</strong></div><button onClick={() => setOverlayExpanded(false)} aria-label="Collapse VIC desktop presence">×</button></div>
          <p className="vic-caption" aria-live="polite">{liveTranscript || lastResponse || "I’m here when you need me."}</p>
          <button className="vic-overlay-action" onClick={handleTalk}>{copy.action}</button>
        </div>}
      </aside>
    </main>
  );
}

function SettingsDialog({ gateway, enrollmentCode, setEnrollmentCode, onSaveGateway, onEnroll, onClose, hasToken, onForget }: { gateway: string; enrollmentCode: string; setEnrollmentCode: (value: string) => void; onSaveGateway: (value: string) => void; onEnroll: (event: FormEvent) => void; onClose: () => void; hasToken: boolean; onForget: () => void }) {
  const [gatewayDraft, setGatewayDraft] = useState(gateway || suggestedGateway);
  return <div className="dialog-backdrop" role="presentation"><section className="settings-dialog panel" role="dialog" aria-modal="true" aria-labelledby="settings-title"><div className="panel-heading"><div><p className="kicker">Private connection</p><h2 id="settings-title">Connect this screen</h2></div><button className="icon-button" aria-label="Close settings" onClick={onClose}>×</button></div><label>VoiceOS gateway URL<input type="url" value={gatewayDraft} onChange={(event) => setGatewayDraft(event.target.value)} placeholder={suggestedGateway} /></label><button className="primary-button" onClick={() => onSaveGateway(gatewayDraft)}>Save and test connection</button><div className="dialog-divider" /><form onSubmit={onEnroll}><label>One-time enrollment code<input inputMode="numeric" autoComplete="one-time-code" value={enrollmentCode} onChange={(event) => setEnrollmentCode(event.target.value)} placeholder="Enter the code from VoiceOS" /></label><button className="primary-button" disabled={!gatewayDraft.trim() || !enrollmentCode.trim()}>{hasToken ? "Replace screen credential" : "Enroll screen"}</button></form>{hasToken && <button className="danger-button" onClick={onForget}>Forget this screen</button>}<p className="dialog-help">The gateway must use HTTPS when this page is opened from a secure URL. Add this site’s exact origin to the gateway’s allowed web origins.</p></section></div>;
}

function FocusPanel({ focus, busy, onRefresh, onLowEnergy, onStart, onSwitch, onCapture, onPromote, onAction }: { focus: FocusSnapshot | null; busy: boolean; onRefresh: () => void; onLowEnergy: () => void; onStart: (minutes: 5 | 20, taskId?: string) => void; onSwitch: (taskId: string) => void; onCapture: (input: { title: string; due_at?: string; importance?: "low" | "normal" | "high" | "critical" }) => Promise<void>; onPromote: (taskId: string) => void; onAction: (sessionId: string, action: "interrupt" | "resume" | "complete", nextAction?: string) => void }) {
  const active = focus?.active_session;
  const interrupted = !active ? focus?.last_interrupted_session : null;
  const recommendation = focus?.recommendation;
  const nextAction = active?.next_action ?? recommendation?.next_action;
  const [clock, setClock] = useState(0);
  const [captureTitle, setCaptureTitle] = useState("");
  const [captureDue, setCaptureDue] = useState("");
  const [captureImportance, setCaptureImportance] = useState<"low" | "normal" | "high" | "critical">("normal");
  const [captureError, setCaptureError] = useState("");
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setClock(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [active]);
  const remaining = active ? focusTimeRemaining(active.updated_at, active.planned_minutes, clock) : null;
  const submitCapture = async (event: FormEvent) => {
    event.preventDefault();
    if (!captureTitle.trim() || busy) return;
    setCaptureError("");
    try {
      await onCapture({
        title: captureTitle.trim(),
        importance: captureImportance,
        ...(captureDue ? { due_at: new Date(captureDue).toISOString() } : {}),
      });
      setCaptureTitle(""); setCaptureDue(""); setCaptureImportance("normal");
    } catch (error) {
      setCaptureError(errorText(error));
    }
  };
  return <section className="focus-screen panel" aria-labelledby="focus-title">
    <div className="panel-heading"><div><p className="kicker">One thing now</p><h2 id="focus-title">Focus with VIC</h2></div><button className="secondary-button" disabled={busy} onClick={onRefresh}>Refresh</button></div>
    <p className="focus-intro">VIC keeps the list short, protects your restart point, and helps you return without judgment. A focus session never marks the whole task done.</p>
    <div className="focus-layout">
      <article className={`focus-now-card ${active ? "active" : ""}`}>
        <header><span>{active ? remaining === "Time-box complete" ? remaining : `${remaining} remaining` : interrupted ? "Your place is saved" : focus?.mode === "low_energy" ? "Low-energy next step" : "Recommended next step"}</span><strong aria-hidden="true">◎</strong></header>
        <p className="focus-only">Only this now</p>
        <h3>{nextAction ?? "No ready task needs your attention."}</h3>
        {recommendation && !active && <p className="focus-context">{recommendation.title}{recommendation.project_title ? ` · ${recommendation.project_title}` : ""}{recommendation.goal_title ? ` · Goal: ${recommendation.goal_title}` : ""}</p>}
        <div className="focus-actions">
          {active ? <><button className="focus-interrupt" disabled={busy} onClick={() => onAction(active.id, "interrupt", active.next_action)}>I got interrupted</button><button className="focus-complete" disabled={busy} onClick={() => onAction(active.id, "complete")}>Done for now</button></> : interrupted ? <button className="focus-primary" disabled={busy} onClick={() => onAction(interrupted.id, "resume")}>Restart for 5 minutes</button> : recommendation ? <><button className="focus-primary" disabled={busy} onClick={() => onStart(5, recommendation.task_id)}>Start 5 minutes</button><button className="focus-secondary" disabled={busy} onClick={() => onStart(20, recommendation.task_id)}>Start 20 minutes</button></> : null}
        </div>
        {interrupted && <p className="focus-restart"><span>Restart point</span>{interrupted.restart_action ?? interrupted.next_action}</p>}
      </article>
      <aside className="focus-rescue-card">
        <p className="kicker">When the day feels heavy</p><h3>Make the next step smaller</h3><p>Show the shortest available action. Starting for five minutes is enough.</p><button disabled={busy} onClick={onLowEnergy}>I’m overwhelmed or low energy</button>
      </aside>
    </div>
    <section className="attention-capture" aria-labelledby="capture-title">
      <div><p className="kicker">Capture without switching</p><h3 id="capture-title">New direction? Park it, don’t pivot.</h3><p>VIC keeps the thought, deadline, and importance outside the active focus queue until you deliberately promote it.</p></div>
      <form onSubmit={(event) => void submitCapture(event)}><label><span>Idea or task</span><input value={captureTitle} onChange={(event) => setCaptureTitle(event.target.value)} placeholder="Something I suddenly want to do…" maxLength={240} /></label><label><span>Deadline, if real</span><input type="datetime-local" value={captureDue} onChange={(event) => setCaptureDue(event.target.value)} /></label><label><span>Importance</span><select value={captureImportance} onChange={(event) => setCaptureImportance(event.target.value as typeof captureImportance)}><option value="low">Low</option><option value="normal">Normal</option><option value="high">High</option><option value="critical">Critical</option></select></label><button disabled={busy || !captureTitle.trim()}>Park without switching</button></form>
      {captureError && <p className="project-error" role="alert">{captureError}</p>}
    </section>
    <section className="focus-priorities" aria-label="Up to three focus priorities">
      <div className="focus-priority-heading"><div><p className="kicker">Protected attention</p><h3>Up to three priorities</h3></div><span>{focus?.priorities.length ?? 0} shown</span></div>
      <div className="focus-priority-grid">{focus?.priorities.length ? focus.priorities.map((priority, index) => <article className={priority.task_id === recommendation?.task_id ? "recommended" : ""} key={priority.task_id}><header><span>{index + 1}</span><small>{focusPriorityLabel(priority)}</small></header><h4>{priority.title}</h4><p>{priority.next_action}</p><footer><span>{priority.project_title ?? "Loose work"}</span>{active && active.task_id !== priority.task_id ? <button disabled={busy} onClick={() => onSwitch(priority.task_id)}>Switch here safely</button> : !active ? <button disabled={busy} onClick={() => onStart(5, priority.task_id)}>Focus 5 min</button> : null}</footer></article>) : <EmptyState text="Add a ready task with a clear next action and VIC will put it here." />}</div>
    </section>
    <section className="parking-lot" aria-label="Idea parking lot"><div className="focus-priority-heading"><div><p className="kicker">Protected from impulse switching</p><h3>Idea Parking Lot</h3></div><span>{focus?.parked.length ?? 0} captured</span></div><div className="parking-list">{focus?.parked.length ? focus.parked.map((idea) => <article key={idea.task_id}><div><strong>{idea.title}</strong><small>{focusPriorityLabel(idea)}</small></div><p>{idea.observable_outcome}</p><button disabled={busy} onClick={() => onPromote(idea.task_id)}>Make actionable</button></article>) : <EmptyState text="New ideas can wait here without stealing the task in front of you." />}</div></section>
  </section>;
}

function MemoryPanel({ memories, sleepCycles, reviewBusy, onScan, onApprove, onSearch, onAdd, onCorrect, onForget }: { memories: VicMemory[]; sleepCycles: SleepCycleReport[]; reviewBusy: boolean; onScan: () => Promise<void>; onApprove: (cycleId: string, changeId: string) => Promise<void>; onSearch: (query?: string) => Promise<void>; onAdd: (content: string, category: string) => Promise<void>; onCorrect: (memory: VicMemory) => Promise<void>; onForget: (memory: VicMemory) => Promise<void> }) {
  const [query, setQuery] = useState("");
  const [content, setContent] = useState("");
  const [category, setCategory] = useState("general");
  const proposals = sleepCycles.flatMap((report) => report.cycle.mode === "dry_run" ? report.changes.filter((change) => change.status === "proposed").map((change) => ({ cycleId: report.cycle.id, change })) : []);
  return <section className="wide-panel panel memory-screen">
    <div className="panel-heading"><div><p className="kicker">Durable personal context</p><h2>VIC memory</h2></div><span className="memory-pill">{memories.length} active</span></div>
    <section className="memory-review"><div className="panel-heading"><div><p className="kicker">Sleep-cycle review</p><h3>Proposed memories</h3></div><button className="secondary-button" disabled={reviewBusy} onClick={() => void onScan()}>{reviewBusy ? "Working…" : "Scan now"}</button></div><p className="memory-review-note">VIC will not save these automatically. Approve only the facts you want carried into future conversations.</p><div className="memory-proposal-list">{proposals.length ? proposals.slice(0, 20).map(({ cycleId, change }) => <article className="memory-proposal-card" key={change.id}><div><span>Proposed</span><small>{Math.round((change.confidence ?? 0) * 100)}% confidence · {formatTime(change.created_at)}</small></div><p>{change.detail}</p><button className="approve" disabled={reviewBusy} onClick={() => void onApprove(cycleId, change.id)}>Remember this</button></article>) : <EmptyState text="No memory proposals are waiting for review." />}</div></section>
    <div className="memory-controls">
      <form onSubmit={(event) => { event.preventDefault(); void onSearch(query); }}><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search what VIC knows…" /><button>Search</button></form>
      <form onSubmit={(event) => { event.preventDefault(); if (!content.trim()) return; void onAdd(content.trim(), category).then(() => setContent("")); }}><input value={content} onChange={(event) => setContent(event.target.value)} placeholder="Add a fact VIC should remember…" maxLength={500} /><select value={category} onChange={(event) => setCategory(event.target.value)}><option value="general">General</option><option value="identity">Identity</option><option value="preference">Preference</option><option value="person">Person</option><option value="project">Project</option><option value="routine">Routine</option><option value="sensitive">Sensitive</option></select><button>Remember</button></form>
    </div>
    <div className="memory-list">{memories.length ? memories.map((memory) => <article className="memory-card" key={memory.id}><div><span>{memory.category}</span><small>{Math.round(memory.confidence * 100)}% confidence · {memory.source.replaceAll("-", " ")}</small></div><p>{memory.content}</p><footer><small>{memory.provenance || "Local conversation"} · {formatTime(memory.updated_at)}</small><button onClick={() => void onCorrect(memory)}>Correct</button><button className="deny" onClick={() => void onForget(memory)}>Forget</button></footer></article>) : <EmptyState text="VIC has no matching durable memories yet. Say “remember that…” or add one above." />}</div>
  </section>;
}

function NavButton({ active, icon, label, onClick }: { active: boolean; icon: string; label: string; onClick: () => void }) {
  return <button className={active ? "active" : ""} onClick={onClick}><span aria-hidden="true">{icon}</span>{label}</button>;
}

function firstSentence(text: string) {
  const normalized = text.trim();
  const sentence = normalized.match(/^.*?[.!?](?=\s|$)/s)?.[0];
  if (sentence) return sentence.trim();
  const firstLine = normalized.split(/\n+/)[0]?.trim() || normalized;
  return firstLine.length > 180 ? `${firstLine.slice(0, 177).trimEnd()}…` : firstLine;
}

function MessageCard({ message, latestVic = false }: { message: Message; latestVic?: boolean }) {
  const collapsible = message.role === "VIC" && !latestVic && firstSentence(message.body) !== message.body.trim();
  const [expanded, setExpanded] = useState(false);
  const showFullReply = latestVic || !collapsible || expanded;
  return <article className={`message ${message.role === "You" ? "user" : "assistant"} ${collapsible ? "message-collapsible" : ""} ${showFullReply ? "message-expanded" : "message-collapsed"}`}>
    <div className="message-label"><strong>{message.role}</strong><span>{message.meta}</span></div>
    {message.images?.map((image) => <figure className="message-image" key={image.url}><img src={image.url} alt={image.filename} /><figcaption>{image.filename}</figcaption></figure>)}
    {collapsible ? <button className="message-reply-toggle" type="button" aria-expanded={expanded} onClick={() => setExpanded((current) => !current)}><span>{showFullReply ? message.body : firstSentence(message.body)}</span><small>{expanded ? "Show less" : "View full reply"}</small></button> : <p>{message.body}</p>}
  </article>;
}

function EmptyState({ text }: { text: string }) {
  return <div className="empty-state"><span aria-hidden="true">⬡</span><p>{text}</p></div>;
}

function CommandTaskSummary({ tasks, onOpenLane }: { tasks: TaskDetail[]; onOpenLane: (lane: "needs_me" | "vic_working" | "review") => void }) {
  const lanes: Array<{ lane: "vic_working" | "needs_me" | "review"; label: string; detail: string }> = [
    { lane: "vic_working", label: "VIC working", detail: "In progress" },
    { lane: "needs_me", label: "Needs you", detail: "Waiting on a decision" },
    { lane: "review", label: "Ready for review", detail: "Outcome to inspect" },
  ];
  return <section className="command-summary panel" aria-label="Task summary">
    <div className="panel-heading"><div><p className="kicker">Command Center</p><h2>What needs attention</h2></div><span className="memory-pill">Live tasks</span></div>
    <div className="command-lanes">{lanes.map(({ lane, label, detail }) => {
      const count = tasks.filter((task) => task.progress.lane === lane).length;
      return <button key={lane} className={`command-lane lane-${lane}`} onClick={() => onOpenLane(lane)}><span>{label}</span><strong>{count}</strong><small>{count === 1 ? detail.replace(/s$/, "") : detail}</small></button>;
    })}</div>
    <p className="command-summary-note">Tap a lane to open its tasks. VIC updates this board as shared work changes.</p>
  </section>;
}

function ProjectsPanel({ projects, tasks, onCreate, onAssign, onRefresh }: { projects: VicProject[]; tasks: TaskDetail[]; onCreate: (title: string) => Promise<void>; onAssign: (taskId: string, projectId: string | null) => Promise<void>; onRefresh: () => void }) {
  const [title, setTitle] = useState("");
  const [creating, setCreating] = useState(false);
  const [movingTaskId, setMovingTaskId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const looseTasks = tasks.filter((detail) => !detail.task.project_id);
  const create = async (event: FormEvent) => {
    event.preventDefault();
    if (!title.trim() || creating) return;
    setCreating(true); setError("");
    try { await onCreate(title.trim()); setTitle(""); }
    catch (caught) { setError(errorText(caught)); }
    finally { setCreating(false); }
  };
  const assign = async (taskId: string, projectId: string | null) => {
    setMovingTaskId(taskId); setError("");
    try { await onAssign(taskId, projectId); }
    catch (caught) { setError(errorText(caught)); }
    finally { setMovingTaskId(null); }
  };
  return <section className="projects-screen panel">
    <div className="panel-heading"><div><p className="kicker">One place for active work</p><h2>Projects with VIC</h2></div><button className="secondary-button" onClick={onRefresh}>Refresh</button></div>
    <p className="projects-intro">Create the project names you want, then use the large selectors to place existing work. Nothing is grouped automatically.</p>
    <form className="project-create" onSubmit={create}><label htmlFor="project-title">New project</label><div><input id="project-title" value={title} onChange={(event) => setTitle(event.target.value)} placeholder="VIC touchscreen, SMB Sentinel, Sunday brunch…" maxLength={160} /><button disabled={creating || !title.trim()}>{creating ? "Creating…" : "Create project"}</button></div></form>
    {error && <p className="project-error" role="alert">{error}</p>}
    <section className="loose-work" aria-labelledby="loose-work-title"><div className="project-section-heading"><div><p className="kicker">Needs a home</p><h3 id="loose-work-title">Loose work</h3></div><span>{looseTasks.length} task{looseTasks.length === 1 ? "" : "s"}</span></div>{looseTasks.length ? <div className="project-task-list">{looseTasks.map((detail) => <ProjectTaskRow key={detail.task.id} detail={detail} projects={projects} busy={movingTaskId === detail.task.id} onAssign={assign} />)}</div> : <EmptyState text="Every open task is connected to a VIC project." />}</section>
    <div className="project-grid">{projects.length ? projects.map((project) => { const projectTasks = tasks.filter((detail) => detail.task.project_id === project.id); return <article className="project-card" key={project.id}><header><div><span>{project.status}</span><h3>{project.title}</h3></div><strong>{projectTasks.length}</strong></header><p>{projectTasks.length ? `${projectTasks.length} open task${projectTasks.length === 1 ? "" : "s"} connected` : "Ready for its first task"}</p><div className="project-task-list">{projectTasks.map((detail) => <ProjectTaskRow key={detail.task.id} detail={detail} projects={projects} busy={movingTaskId === detail.task.id} onAssign={assign} />)}</div></article>; }) : <EmptyState text="Create your first project, then connect the work VIC already knows about." />}</div>
  </section>;
}

function ProjectTaskRow({ detail, projects, busy, onAssign }: { detail: TaskDetail; projects: VicProject[]; busy: boolean; onAssign: (taskId: string, projectId: string | null) => Promise<void> }) {
  return <article className="project-task-row"><div><strong>{detail.task.title}</strong><small>{detail.progress.lane.replaceAll("_", " ")} · {detail.task.estimated_minutes} min</small></div><label><span className="visually-hidden">Project for {detail.task.title}</span><select aria-label={`Project for ${detail.task.title}`} disabled={busy} value={detail.task.project_id ?? ""} onChange={(event) => void onAssign(detail.task.id, event.target.value || null)}><option value="">Loose work</option>{projects.map((project) => <option key={project.id} value={project.id}>{project.title}</option>)}</select></label></article>;
}

function TaskBoard({ projects, tasks, activity, filter, onFilter, onRefresh, onAttention, onStart }: { projects: VicProject[]; tasks: TaskDetail[]; activity: AgentActivity[]; filter: "all" | "needs_me" | "vic_working" | "review"; onFilter: (value: "all" | "needs_me" | "vic_working" | "review") => void; onRefresh: () => void; onAttention: (taskId: string, input: { due_at: string | null; importance: TaskDetail["task"]["importance"] }) => Promise<void>; onStart: (input: { title: string; observable_outcome: string; estimated_minutes: number; project_id?: string }) => Promise<void> }) {
  const [title, setTitle] = useState("");
  const [outcome, setOutcome] = useState("");
  const [minutes, setMinutes] = useState(20);
  const [projectId, setProjectId] = useState("");
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState("");
  const visible = tasks.filter((task) => filter === "all" || task.progress.lane === filter);
  const count = (lane: TaskDetail["progress"]["lane"]) => tasks.filter((task) => task.progress.lane === lane).length;
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!title.trim() || !outcome.trim() || starting) return;
    setStarting(true); setStartError("");
    try { await onStart({ title: title.trim(), observable_outcome: outcome.trim(), estimated_minutes: minutes, ...(projectId ? { project_id: projectId } : {}) }); setTitle(""); setOutcome(""); setMinutes(20); }
    catch (error) { setStartError(errorText(error)); }
    finally { setStarting(false); }
  };
  return <section className="task-board panel"><div className="panel-heading"><div><p className="kicker">Human + agent execution</p><h2>Task responsibility board</h2></div><button className="secondary-button" onClick={onRefresh}>Refresh</button></div><form className="task-intake" onSubmit={submit}><div><label htmlFor="task-title">What should VIC work on?</label><input id="task-title" value={title} onChange={(event) => setTitle(event.target.value)} placeholder="Build the customer follow-up workflow" /></div><div><label htmlFor="task-outcome">What does done look like?</label><input id="task-outcome" value={outcome} onChange={(event) => setOutcome(event.target.value)} placeholder="A tested workflow is ready for my review" /></div><div><label htmlFor="task-project">Project</label><select id="task-project" value={projectId} onChange={(event) => setProjectId(event.target.value)}><option value="">Loose work</option>{projects.map((project) => <option key={project.id} value={project.id}>{project.title}</option>)}</select></div><div className="task-duration"><label htmlFor="task-minutes">Estimate</label><input id="task-minutes" type="number" min="1" max="1440" value={minutes} onChange={(event) => setMinutes(Math.max(1, Number(event.target.value) || 1))} /><span>min</span></div><button disabled={starting || !title.trim() || !outcome.trim()}>{starting ? "Starting…" : "Start task with VIC"}</button>{startError && <p className="task-intake-error">{startError}</p>}</form><p className="task-intake-note">VIC begins safe research, drafting, planning, or project inspection immediately. Approvals remain required for external or consequential actions.</p><div className="task-rollups"><button className={filter === "needs_me" ? "active" : ""} onClick={() => onFilter("needs_me")}><span>Needs me</span><strong>{count("needs_me")}</strong></button><button className={filter === "vic_working" ? "active" : ""} onClick={() => onFilter("vic_working")}><span>VIC working</span><strong>{count("vic_working")}</strong></button><button className={filter === "review" ? "active" : ""} onClick={() => onFilter("review")}><span>Ready for review</span><strong>{count("review")}</strong></button><button className={filter === "all" ? "active" : ""} onClick={() => onFilter("all")}><span>All open</span><strong>{tasks.length}</strong></button></div><div className="task-grid">{visible.length ? visible.map((detail) => { const live = activity.filter((item) => item.taskId === detail.task.id).slice(0, 3); const recorded = detail.activity?.filter((item) => item.event_type === "task.progress.recorded").slice(-3).reverse() ?? []; return <article className={`task-card lane-${detail.progress.lane}`} key={detail.task.id}><div className="task-card-head"><span>{detail.progress.lane.replaceAll("_", " ")}</span><strong>{detail.progress.total_steps ? `${detail.progress.completed_steps}/${detail.progress.total_steps} steps` : "No steps"}</strong></div><h3>{detail.task.title}</h3><p>{detail.task.observable_outcome}</p><div className="task-handoff"><small>{detail.progress.lane === "vic_working" ? "VIC NEXT ACTION" : detail.progress.lane === "review" ? "READY FOR REVIEW" : "YOUR NEXT ACTION"}</small><strong>{detail.progress.lane === "vic_working" ? detail.progress.next_vic_action || "Continue safe work" : detail.progress.next_user_action || "Review with VIC"}</strong></div>{(live.length > 0 || recorded.length > 0) && <div className="task-updates"><small>PROGRESS UPDATES</small>{live.map((item) => <div className="task-update live" key={item.id}><span className="thinking-pulse" /><p><strong>{item.label}</strong>{item.detail && <small>{item.detail}</small>}</p></div>)}{live.length === 0 && recorded.map((item, index) => <div className="task-update" key={item.id ?? `${item.occurred_at}-${index}`}><span>✓</span><p>{String(item.payload.summary ?? "VIC recorded progress")}</p></div>)}</div>}<div className="task-steps">{detail.steps.slice(0, 5).map((step) => <div key={step.id}><span>{step.status === "completed" ? "✓" : "○"}</span><p>{step.title}</p><small>{step.owner}</small></div>)}</div><TaskAttentionEditor detail={detail} onSave={onAttention} /><footer><span>VIC {detail.progress.vic_status.replaceAll("_", " ")}</span><span>{detail.progress.open_blockers} blockers</span><span>{detail.artifacts.length} artifacts</span></footer></article>; }) : <EmptyState text="No tasks are in this responsibility lane." />}</div></section>;
}

function TaskAttentionEditor({ detail, onSave }: { detail: TaskDetail; onSave: (taskId: string, input: { due_at: string | null; importance: TaskDetail["task"]["importance"] }) => Promise<void> }) {
  const [due, setDue] = useState(dateTimeInputValue(detail.task.due_at));
  const [importance, setImportance] = useState(detail.task.importance);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (saving) return;
    setSaving(true); setError("");
    try { await onSave(detail.task.id, { due_at: due ? new Date(due).toISOString() : null, importance }); }
    catch (problem) { setError(errorText(problem)); }
    finally { setSaving(false); }
  };
  return <form className="task-attention" onSubmit={(event) => void save(event)}><p><strong>When should this rise?</strong><span>Real deadline + importance</span></p><label><span>Deadline</span><input type="datetime-local" value={due} onChange={(event) => setDue(event.target.value)} /></label><label><span>Importance</span><select value={importance} onChange={(event) => setImportance(event.target.value as typeof importance)}><option value="low">Low</option><option value="normal">Normal</option><option value="high">High</option><option value="critical">Critical</option></select></label><button disabled={saving}>{saving ? "Saving…" : "Save"}</button>{error && <small role="alert">{error}</small>}</form>;
}

function SkillProposalPanel({ proposals, onDecision, onRefresh }: { proposals: SkillProposal[]; onDecision: (proposal: SkillProposal, approve: boolean) => void; onRefresh: () => void }) {
  return <section className="skill-proposals-panel panel"><div className="panel-heading"><div><p className="kicker">Reviewed self-improvement</p><h2>Skill proposals</h2></div><button className="secondary-button" onClick={onRefresh}>Refresh</button></div><p className="proposal-intro">{proposals.length ? `${proposals.length} evidence-backed proposal${proposals.length === 1 ? "" : "s"} waiting for your decision.` : "Nothing is waiting for review. VoiceOS never enables a generated skill silently."}</p><div className="skill-proposal-list">{proposals.map((proposal) => <article className="skill-proposal-card" key={proposal.id}><div className="skill-proposal-title"><div><span className="proposal-version">Version {proposal.version}</span><h3>{proposal.name}</h3></div><span className="proposal-status">Review required</span></div><div className="proposal-facts"><span><strong>{proposal.evidence.length}</strong> successful audit turns</span><span><strong>{proposal.required_capabilities.length}</strong> typed capabilities</span></div><div className="capability-list">{proposal.required_capabilities.map((capability, index) => <code key={`${String(capability)}-${index}`}>{String(capability)}</code>)}</div><details><summary>Inspect proposed procedure</summary><pre>{proposal.content}</pre></details><details><summary>Inspect source evidence</summary><pre>{JSON.stringify(proposal.evidence, null, 2)}</pre></details><div className="proposal-actions"><button className="deny" onClick={() => onDecision(proposal, false)}>Reject</button><button className="approve" onClick={() => onDecision(proposal, true)}>Approve version</button></div><p className="proposal-safety">Approval records this version for later permissioned use. The proposal itself cannot execute.</p></article>)}</div></section>;
}

function SkillCatalogPanel({ skills, usages, onDisable, onFeedback }: { skills: SkillProposal[]; usages: SkillUsage[]; onDisable: (skill: SkillProposal) => void; onFeedback: (usage: SkillUsage, correct: boolean) => void }) {
  return <section className="skill-proposals-panel panel"><div className="panel-heading"><div><p className="kicker">Active capability library</p><h2>VIC skills</h2></div><span className="healthy-label">{skills.length} active</span></div><div className="skill-proposal-list">{skills.map((skill) => <article className="skill-proposal-card" key={skill.id}><div className="skill-proposal-title"><div><span className="proposal-version">Version {skill.version}</span><h3>{skill.name}</h3></div><span className="provider-state green">Active</span></div><div className="capability-list">{skill.required_capabilities.length ? skill.required_capabilities.map((capability, index) => <code key={`${String(capability)}-${index}`}>{String(capability)}</code>) : <code>coordination procedure</code>}</div><details><summary>Inspect procedure</summary><pre>{skill.content}</pre></details><button className="deny" onClick={() => onDisable(skill)}>Disable skill</button></article>)}</div><div className="panel-heading"><div><p className="kicker">Learning from real use</p><h3>Recent skill activity</h3></div></div><div className="skill-proposal-list">{usages.length ? usages.slice(0, 10).map((usage) => <article className="skill-proposal-card" key={usage.id}><div className="skill-proposal-title"><div><span className="proposal-version">Version {usage.skill_version}</span><h3>{usage.skill_name}</h3></div><span className="proposal-status">{usage.outcome}</span></div>{usage.feedback ? <p className="proposal-safety">Reviewed: {usage.feedback}</p> : <div className="proposal-actions"><button className="approve" onClick={() => onFeedback(usage, true)}>Used correctly</button><button className="deny" onClick={() => onFeedback(usage, false)}>Used incorrectly</button></div>}</article>) : <EmptyState text="VIC has not used an approved typed workflow since tracking was enabled." />}</div></section>;
}

function ComponentRegistryPanel({ registry }: { registry: ComponentRegistry | null }) {
  return <section className="component-registry-panel panel wide"><div className="panel-heading"><div><p className="kicker">Integration spine</p><h2>VoiceOS components</h2></div><span className="healthy-label">Contract v{registry?.schema_version ?? "—"}</span></div>{registry ? <div className="component-registry-list">{registry.components.map((component) => { const transport = typeof component.integration.transport === "string" ? component.integration.transport.replaceAll("_", " ") : null; return <article className="component-registry-card" key={component.id}><header><div><strong>{component.display_name}</strong><small>{component.role.replaceAll("_", " ")}</small></div><span className={`component-lifecycle lifecycle-${component.lifecycle}`}>{component.lifecycle}</span></header><p>{component.capabilities.map((capability) => capability.replaceAll("_", " ")).join(" · ")}</p>{transport && <footer>{transport}</footer>}</article>; })}</div> : <EmptyState text="Connect to VoiceOS to load the Touch, VIC, and VIC Console integration registry." />}</section>;
}

function ProviderPanel({ providers, active, wide = false }: { providers: Provider[]; active?: string; wide?: boolean }) {
  const displayProviders = providers.length ? providers : [
    { name: "ollama", role: "Fast local voice" },
    { name: "ollama-deep", role: "Deep local reasoning" },
    { name: "codex-sol", role: "Highest confidence" },
  ];
  return <section className={`provider-panel panel ${wide ? "wide" : ""}`}><div className="panel-heading"><div><p className="kicker">Reasoning fabric</p><h2>Model providers</h2></div></div><div className="provider-list">{displayProviders.slice(0, 5).map((provider) => { const selected = provider.name === active; return <div className="provider-row" key={provider.name}><span className={`provider-glyph ${selected ? "green" : "cyan"}`} aria-hidden="true">{selected ? "✦" : "◎"}</span><div><strong>{providerLabel(provider.name)}</strong><small>{provider.role ?? "VIC provider"}</small></div><span className={`provider-state ${selected ? "green" : provider.configured === false ? "amber" : "cyan"}`}>{selected ? "Active" : provider.configured === false ? "Offline" : "Ready"}</span></div>; })}</div></section>;
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

function focusTimeRemaining(startedAt: string, plannedMinutes: number, now: number) {
  if (now === 0) return `${plannedMinutes}:00`;
  const started = Date.parse(startedAt);
  if (!Number.isFinite(started)) return `${plannedMinutes} min`;
  const seconds = Math.max(0, Math.ceil((started + plannedMinutes * 60_000 - now) / 1_000));
  if (seconds === 0) return "Time-box complete";
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

function focusPriorityLabel(priority: FocusPriority) {
  const urgency = priority.urgency === "unscheduled" ? "No deadline" : priority.urgency.replaceAll("_", " ");
  return `${urgency} · ${priority.importance} · ${priority.estimated_minutes} min`;
}

function dateTimeInputValue(value: string | null) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return "";
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

function formatTime(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

function percent(value?: number) {
  return typeof value === "number" ? `${value.toFixed(1)}%` : "—";
}
