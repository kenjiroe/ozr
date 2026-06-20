export type SessionStatus = "running" | "pending_approval" | "completed" | "failed";

export interface PendingApproval {
  plan_id: string;
  tool: string;
  action_kind: string;
  risk_tier: string;
  params: string;
}

export interface SessionView {
  session_id: string;
  status: SessionStatus;
  prompt: string;
  pending?: PendingApproval;
  result?: string;
  error?: string;
}

export interface RunResponse {
  session_id: string;
  status: SessionStatus;
  message: string;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export class OzrApiClient {
  constructor(private readonly baseUrl: string) {}

  get base(): string {
    return this.baseUrl;
  }

  async health(): Promise<boolean> {
    try {
      const response = await fetch(`${this.baseUrl}/health`);
      return response.ok && (await response.text()) === "ok";
    } catch {
      return false;
    }
  }

  async waitReady(maxAttempts = 60, intervalMs = 200): Promise<void> {
    for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
      if (await this.health()) {
        return;
      }
      await sleep(intervalMs);
    }
    throw new Error(`ozr API not ready at ${this.baseUrl}`);
  }

  async run(prompt: string): Promise<RunResponse> {
    let response: Response;
    try {
      response = await fetch(`${this.baseUrl}/v1/run`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ prompt }),
      });
    } catch (error) {
      throw new Error(
        `cannot reach ozr API at ${this.baseUrl} (${error instanceof Error ? error.message : "network error"}). Restart the GUI after \`cargo build\`.`,
      );
    }
    if (!response.ok) {
      const body = await response.json().catch(() => ({}));
      throw new Error(body.error ?? response.statusText);
    }
    return response.json();
  }

  async getSession(sessionId: string): Promise<SessionView> {
    const response = await fetch(`${this.baseUrl}/v1/session/${sessionId}`);
    if (!response.ok) {
      const body = await response.json().catch(() => ({}));
      throw new Error(body.error ?? response.statusText);
    }
    return response.json();
  }

  async approve(
    sessionId: string,
    decision: "approve" | "deny",
    reason?: string,
  ): Promise<SessionView> {
    const response = await fetch(`${this.baseUrl}/v1/session/${sessionId}/approve`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        decision,
        reason: reason ?? `${decision} via ozr-gui`,
      }),
    });
    if (!response.ok) {
      const body = await response.json().catch(() => ({}));
      throw new Error(body.error ?? response.statusText);
    }
    return response.json();
  }
}

export async function pollSessionUntil(
  client: OzrApiClient,
  sessionId: string,
  wantStatus: SessionStatus,
  maxAttempts = 80,
  intervalMs = 250,
): Promise<SessionView> {
  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    await sleep(intervalMs);
    const view = await client.getSession(sessionId);
    if (view.status === wantStatus) {
      return view;
    }
    if (view.status === "failed") {
      throw new Error(view.error ?? "session failed");
    }
    if (view.status === "completed" && wantStatus !== "completed") {
      throw new Error("session completed before reaching expected approval gate");
    }
  }
  throw new Error(`timed out waiting for session status: ${wantStatus}`);
}
