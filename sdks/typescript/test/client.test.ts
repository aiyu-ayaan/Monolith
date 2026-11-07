import { describe, expect, it, vi } from "vitest";
import {
  MonolithClient,
  MonolithError,
  createClient,
} from "../src/index.js";

describe("MonolithClient", () => {
  it("initializes with default options", () => {
    const client = createClient({
      url: "https://api.example.com",
      app: "testapp",
    });

    expect(client).toBeInstanceOf(MonolithClient);
    expect(client.http.baseUrl).toBe("https://api.example.com");
    expect(client.http.app).toBe("testapp");
    expect(client.http.getToken()).toBeNull();
  });

  it("builds correct URLs with query parameters", () => {
    const client = createClient({
      url: "https://api.example.com/",
      app: "testapp",
    });

    const url = client.http.buildUrl("/test", {
      filter: "status = 'active'",
      limit: 10,
      tags: ["a", "b"],
      empty: undefined,
    });

    expect(url).toBe(
      "https://api.example.com/test?filter=status+%3D+%27active%27&limit=10&tags=a&tags=b",
    );
  });

  it("handles collection CRUD requests", async () => {
    const mockFetch = vi.fn().mockImplementation(async (url: string, init: RequestInit) => {
      const method = init.method ?? "GET";

      if (url.includes("/collections/todos/records:count")) {
        return new Response(JSON.stringify({ count: 42 }), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      }

      if (url.includes("/collections/todos/records/rec_123") && method === "GET") {
        return new Response(
          JSON.stringify({ id: "rec_123", title: "Task 1", created: "2026-01-01", updated: "2026-01-01", _v: 1 }),
          { status: 200, headers: { "content-type": "application/json" } },
        );
      }

      if (url.includes("/collections/todos/records") && method === "POST") {
        const body = JSON.parse(init.body as string);
        return new Response(
          JSON.stringify({ id: "rec_new", ...body, created: "2026-01-01", updated: "2026-01-01", _v: 1 }),
          { status: 201, headers: { "content-type": "application/json" } },
        );
      }

      if (url.includes("/collections/todos/records/rec_123") && method === "PATCH") {
        const body = JSON.parse(init.body as string);
        return new Response(
          JSON.stringify({ id: "rec_123", ...body, updated: "2026-01-02", _v: 2 }),
          { status: 200, headers: { "content-type": "application/json" } },
        );
      }

      if (url.includes("/collections/todos/records/rec_123") && method === "DELETE") {
        return new Response(null, { status: 204 });
      }

      if (url.includes("/collections/todos/records") && method === "GET") {
        return new Response(
          JSON.stringify({
            items: [{ id: "rec_1", title: "Item 1" }],
            total: 1,
            cursor: "cur_1",
          }),
          { status: 200, headers: { "content-type": "application/json" } },
        );
      }

      return new Response("Not found", { status: 404 });
    });

    const client = createClient({
      url: "https://api.example.com",
      app: "testapp",
      fetch: mockFetch,
    });

    const todos = client.collection("todos");

    // List
    const listRes = await todos.list({ filter: "done = false", limit: 5 });
    expect(listRes.items).toHaveLength(1);
    expect(listRes.items[0].id).toBe("rec_1");

    // Count
    const count = await todos.count();
    expect(count).toBe(42);

    // GetOne
    const item = await todos.getOne("rec_123");
    expect(item.id).toBe("rec_123");

    // Create
    const created = await todos.create({ title: "New Task" } as any);
    expect(created.id).toBe("rec_new");

    // Update
    const updated = await todos.update("rec_123", { title: "Updated Task" } as any);
    expect(updated.title).toBe("Updated Task");

    // Delete
    const deleted = await todos.delete("rec_123");
    expect(deleted).toBe(true);
  });

  it("handles problem+json errors", async () => {
    const mockFetch = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          type: "validation_error",
          title: "Validation Failed",
          status: 400,
          detail: "The title field is required",
          requestId: "req_xyz",
          errors: [{ field: "title", code: "required", message: "must not be empty" }],
        }),
        {
          status: 400,
          headers: {
            "content-type": "application/problem+json",
            "x-request-id": "req_xyz",
          },
        },
      ),
    );

    const client = createClient({
      url: "https://api.example.com",
      app: "testapp",
      fetch: mockFetch,
    });

    try {
      await client.collection("todos").create({});
      expect.unreachable("should have thrown MonolithError");
    } catch (e) {
      expect(e).toBeInstanceOf(MonolithError);
      const err = e as MonolithError;
      expect(err.status).toBe(400);
      expect(err.type).toBe("validation_error");
      expect(err.requestId).toBe("req_xyz");
      expect(err.getFieldError("title")).toBe("must not be empty");
    }
  });

  it("manages auth sessions and notifies listeners", async () => {
    const mockFetch = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ token: "jwt_token_123", user: { id: "u_1" } }), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );

    const client = createClient({
      url: "https://api.example.com",
      app: "testapp",
      fetch: mockFetch,
    });

    const listener = vi.fn();
    const unsub = client.auth("users").onChange(listener);

    await client.auth("users").signInWithPassword("test@example.com", "secret123");

    expect(client.http.getToken()).toBe("jwt_token_123");
    expect(listener).toHaveBeenCalledWith("jwt_token_123");

    client.auth("users").signOut();
    expect(client.http.getToken()).toBeNull();
    expect(listener).toHaveBeenCalledWith(null);

    unsub();
  });

  it("generates file URLs correctly", () => {
    const client = createClient({
      url: "https://api.example.com",
      app: "testapp",
    });

    const url = client.files.getURL("avatars", "user1.png", { thumb: "100x100" });
    expect(url).toBe(
      "https://api.example.com/api/v1/apps/testapp/storage/buckets/avatars/objects/user1.png?thumb=100x100",
    );
  });
});

