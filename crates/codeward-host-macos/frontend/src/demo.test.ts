import { expect, test } from "vitest";
import { createDemoAdapter, installDemo } from "./demo";
import type { Details, Page, Operation } from "./native";
async function opened() {
  const api = createDemoAdapter();
  const s = await api("open_managed_vault");
  await api("unlock_managed_vault", { ...s, password: "sample" });
  const page = await api<Page>("list_credentials", s);
  return { api, page };
}
test("demo starts locked and rejects credential access until explicitly unlocked", async () => {
  const api = createDemoAdapter();
  await expect(
    api("list_credentials", { epoch: 0, revocation: 0 }),
  ).rejects.toBe("locked");
  const { api: unlocked, page } = await opened();
  expect(page.summaries).toHaveLength(3);
  expect(
    (
      await unlocked<Details>("get_credential_details", {
        ...page,
        id: page.summaries[0].id,
      })
    ).notes,
  ).toContain("sample");
});
test("demo writes only commit on operation polling and Keep preserves passwords", async () => {
  const { api, page } = await opened();
  const form = {
    title: "Demo created",
    username: "sample",
    url: "https://example.test",
    notes: "sample note",
    password: { mode: "provided", value: "SAMPLE SECRET" },
  };
  const queued = await api<Operation>("create_credential", { ...page, form });
  expect(queued.state).toBe("saving");
  expect((await api<Page>("list_credentials", page)).summaries).toHaveLength(3);
  expect((await api<Operation>("get_credential_operation", queued)).state).toBe(
    "saved",
  );
  const next = await api<Page>("list_credentials", page);
  const created = next.summaries.find((x) => x.title === form.title)!;
  const edited = await api<Operation>("update_credential", {
    ...next,
    id: created.id,
    form: { ...form, title: "Edited", password: { mode: "keep" } },
  });
  await api("get_credential_operation", edited);
  const updated = await api<Page>("list_credentials", next);
  const reveal = await api<{ password: string }>("reveal_credential_password", {
    ...updated,
    id: created.id,
  });
  expect(reveal.password).toBe("SAMPLE SECRET");
  const removed = await api<Operation>("delete_credential", {
    ...updated,
    id: created.id,
  });
  await api("get_credential_operation", removed);
  expect((await api<Page>("list_credentials", updated)).summaries).toHaveLength(
    3,
  );
});
test("demo rejects stale selections, locks, and resets in a new adapter", async () => {
  const { api, page } = await opened();
  await expect(
    api("get_credential_details", {
      ...page,
      revision: "0",
      id: page.summaries[0].id,
    }),
  ).rejects.toBe("stale");
  await api("lock_managed_vault", page);
  await expect(
    api("get_credential_details", { ...page, id: page.summaries[0].id }),
  ).rejects.toBe("locked");
  const fresh = await opened();
  expect(fresh.page.summaries).toHaveLength(3);
});
test("install never overrides a native Tauri connection", () => {
  const native = { core: { invoke: async () => ({ native: true }) } };
  const target = { __TAURI__: native };
  expect(installDemo(target as never)).toBe(false);
  expect(target.__TAURI__).toBe(native);
});

test("demo generated references replace safely and commit without plaintext form", async () => {
  const { api, page } = await opened();
  const options = {
    length: 24,
    uppercase: true,
    lowercase: true,
    digits: true,
    symbols: true,
  };
  const first = await api<any>("generate_password_draft", { ...page, options });
  const second = await api<any>("generate_password_draft", {
    ...page,
    options,
  });
  await expect(api("discard_password_draft", first)).rejects.toBe("stale");
  await expect(
    api("generate_password_draft", {
      ...page,
      options: { ...options, length: 1 },
    }),
  ).rejects.toBe("invalid_input");
  const preview = await api<any>("reveal_password_draft", second);
  expect(preview.password).toHaveLength(24);
  const operation = await api<Operation>("create_credential", {
    ...page,
    form: {
      title: "Generated demo",
      username: "sample",
      url: "",
      notes: "",
      password: { mode: "generated", draft: second.draft },
    },
  });
  await api("get_credential_operation", operation);
  const next = await api<Page>("list_credentials", page);
  const created = next.summaries.find((x) => x.title === "Generated demo")!;
  expect(
    (await api<any>("reveal_credential_password", { ...next, id: created.id }))
      .password,
  ).toBe(preview.password);
  await expect(api("reveal_password_draft", second)).rejects.toBe("stale");
});

test("demo native search uses independent fields and committed identity", async () => {
  const { api, page } = await opened();
  expect(
    (
      await api<Page>("search_credentials", {
        ...page,
        query: "SAMPLE.DEVELOPER",
        cursor: null,
      })
    ).summaries.map((x) => x.id),
  ).toEqual(["sample-code"]);
  expect(
    (
      await api<Page>("search_credentials", {
        ...page,
        query: "repository sample",
        cursor: null,
      })
    ).summaries,
  ).toEqual([]);
  await expect(
    api("search_credentials", {
      ...page,
      query: "é".repeat(2049),
      cursor: null,
    }),
  ).rejects.toBe("invalid_input");
  const operation = await api<Operation>("create_credential", {
    ...page,
    form: {
      title: "Sample mail",
      username: "another",
      url: "",
      notes: "",
      password: { mode: "provided", value: "synthetic" },
    },
  });
  expect((operation as any).credential_id).toBeNull();
  const saved = await api<Operation>("get_credential_operation", operation);
  expect((saved as any).credential_id).toBeTruthy();
  expect(
    (
      await api<Details>("get_credential_details", {
        ...(await api<Page>("list_credentials", page)),
        id: (saved as any).credential_id,
      })
    ).username,
  ).toBe("another");
});
