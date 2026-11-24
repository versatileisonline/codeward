export type Status = {
  epoch: number;
  revocation: number;
  state: string;
  error?: string;
  close_requested?: boolean;
};
declare global {
  interface Window {
    __TAURI__?: {
      core: {
        invoke: <T>(
          command: string,
          body?: Record<string, unknown>,
        ) => Promise<T>;
      };
    };
  }
}
export function invoke<T>(
  command: string,
  body: Record<string, unknown> = {},
): Promise<T> {
  return (
    window.__TAURI__?.core.invoke<T>(command, body) ??
    Promise.reject("desktop_required")
  );
}
export type Scope = {
  epoch: number;
  revocation: number;
  generation: string;
  revision: string;
};
export type Summary = { id: string; title: string; username: string };
export type Page = Scope & { summaries: Summary[]; next: string | null };
export type Details = Scope &
  Summary & {
    url: string;
    notes: string;
    created_at: string;
    updated_at: string;
  };
export type Form = {
  title: string;
  username: string;
  url: string;
  notes: string;
  password:
    | { mode: "keep" | "provided"; value?: string }
    | { mode: "generated"; draft: string };
};
export type Operation = {
  epoch: number;
  revocation: number;
  operation: string;
  state: string;
  credential_id?: string | null;
  error?: string;
};

export type GeneratedDraft = Scope & { draft: string };
export type PasswordOptions = {
  length: number;
  uppercase: boolean;
  lowercase: boolean;
  digits: boolean;
  symbols: boolean;
};
