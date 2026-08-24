export const DEFAULT_VOICEOS_GATEWAY = "http://127.0.0.1:8787";

export const VOICEOS_ENDPOINTS = {
  bootstrap: "/v1/client/bootstrap",
  health: "/v1/health",
  providers: "/v1/providers",
  systemHealth: "/v1/tools/system.health",
  events: "/v1/events",
  focus: "/v1/focus",
} as const;

export type FocusSession = {
  id: string;
  owner_id: string;
  task_id: string;
  step_id: string | null;
  mode: "normal" | "five_minute" | "low_energy" | "restart";
  planned_minutes: number;
  status: "active" | "interrupted" | "completed" | "cancelled";
  next_action: string;
  interruption_note: string | null;
  restart_action: string | null;
  reflection: string | null;
  started_at: string;
  updated_at: string;
  ended_at: string | null;
};

export type FocusPriority = {
  task_id: string;
  title: string;
  observable_outcome: string;
  estimated_minutes: number;
  due_at: string | null;
  importance: "low" | "normal" | "high" | "critical";
  urgency: "overdue" | "due_today" | "due_soon" | "due_this_week" | "scheduled" | "unscheduled";
  status: string;
  next_action: string;
  project_title: string | null;
  goal_title: string | null;
};

export type FocusSnapshot = {
  mode: "normal" | "five_minute" | "low_energy" | "restart";
  active_session: FocusSession | null;
  priorities: FocusPriority[];
  recommendation: FocusPriority | null;
  last_interrupted_session: FocusSession | null;
  parked: FocusPriority[];
};

export type SystemComponent = {
  id: string;
  display_name: string;
  role: string;
  lifecycle: "production" | "registered" | "preview" | "retired";
  integration: Record<string, unknown>;
  capabilities: string[];
};

export type ComponentRegistry = {
  schema_version: number;
  system_id: string;
  roles: {
    backend_control_plane: string;
    voice_interface_controller: string;
    touchscreen_system_interface: string;
  };
  components: SystemComponent[];
};

export type ClientBootstrap = {
  contract_version: number;
  device_id: string;
  authentication: { scheme: "bearer" };
  component_registry: ComponentRegistry;
  endpoints: {
    bootstrap: string;
    conversation: string;
    conversation_events: string;
    turn: string;
  };
  transport: {
    private_network_required: boolean;
    tls_required: boolean;
  };
};

export function cleanGateway(value: string) {
  return value.trim().replace(/\/+$/, "");
}

export class VoiceOSClient {
  constructor(
    private readonly gateway: string,
    private readonly token: string,
  ) {}

  async fetch(path: string, init: RequestInit = {}): Promise<Response> {
    const baseUrl = cleanGateway(this.gateway);
    if (!baseUrl) {
      throw new Error("Enter the VoiceOS gateway URL in Connection settings.");
    }
    const headers = new Headers(init.headers);
    if (!headers.has("Accept")) headers.set("Accept", "application/json");
    if (this.token) headers.set("Authorization", `Bearer ${this.token}`);
    return fetch(`${baseUrl}${path}`, { ...init, headers, cache: "no-store" });
  }

  async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const headers = new Headers(init.headers);
    if (init.body && !headers.has("Content-Type")) {
      headers.set("Content-Type", "application/json");
    }
    const response = await this.fetch(path, { ...init, headers });
    const payload = await response.json().catch(() => ({}));
    if (!response.ok) {
      const reason = typeof payload.error === "string"
        ? payload.error.replaceAll("_", " ")
        : `HTTP ${response.status}`;
      throw new Error(`VoiceOS gateway: ${reason}`);
    }
    return payload as T;
  }

  bootstrap(): Promise<ClientBootstrap> {
    return this.request<ClientBootstrap>(VOICEOS_ENDPOINTS.bootstrap);
  }

  async focus(mode: FocusSnapshot["mode"] = "normal"): Promise<FocusSnapshot> {
    const payload = await this.request<{ focus: FocusSnapshot }>(
      `${VOICEOS_ENDPOINTS.focus}?mode=${encodeURIComponent(mode)}`,
    );
    return payload.focus;
  }

  async startFocus(input: {
    task_id?: string;
    mode?: FocusSnapshot["mode"];
    planned_minutes?: number;
  }): Promise<FocusSnapshot> {
    const payload = await this.request<{ focus: FocusSnapshot }>("/v1/focus/sessions", {
      method: "POST",
      body: JSON.stringify(input),
    });
    return payload.focus;
  }

  async actFocus(
    sessionId: string,
    input: {
      action: "interrupt" | "resume" | "complete";
      note?: string;
      restart_action?: string;
      reflection?: string;
      planned_minutes?: number;
    },
  ): Promise<FocusSnapshot> {
    const payload = await this.request<{ focus: FocusSnapshot }>(
      `/v1/focus/sessions/${encodeURIComponent(sessionId)}/actions`,
      { method: "POST", body: JSON.stringify(input) },
    );
    return payload.focus;
  }

  async switchFocus(taskId: string, plannedMinutes = 5): Promise<FocusSnapshot> {
    const payload = await this.request<{ focus: FocusSnapshot }>("/v1/focus/switch", {
      method: "POST",
      body: JSON.stringify({ task_id: taskId, planned_minutes: plannedMinutes }),
    });
    return payload.focus;
  }

  async captureFocus(input: {
    title: string;
    details?: string;
    estimated_minutes?: number;
    due_at?: string;
    importance?: FocusPriority["importance"];
  }): Promise<FocusSnapshot> {
    const payload = await this.request<{ focus: FocusSnapshot }>("/v1/focus/captures", {
      method: "POST",
      body: JSON.stringify(input),
    });
    return payload.focus;
  }
}
