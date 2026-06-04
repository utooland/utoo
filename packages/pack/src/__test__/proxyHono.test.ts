/**
 * Unit tests for Hono dev server HTTP proxy (proxy-hono.ts).
 */

import type { ProxyRule } from "@utoo/pack-shared";
import { Hono } from "hono";
import { describe, expect, it } from "vitest";
import {
  applyPathRewrite,
  createHttpProxyMiddleware,
  doesContextMatchUrl,
  prepareProxyRequest,
  ruleMatchesUrl,
} from "../core/proxyHono";

describe("doesContextMatchUrl", () => {
  it("matches path prefix", () => {
    expect(doesContextMatchUrl("/api", "/api")).toBe(true);
    expect(doesContextMatchUrl("/api", "/api/posts/1")).toBe(true);
    expect(doesContextMatchUrl("/api", "/api")).toBe(true);
    expect(doesContextMatchUrl("/api", "/other")).toBe(false);
    expect(doesContextMatchUrl("/api", "/ap")).toBe(false);
  });

  it("matches regex when context starts with ^", () => {
    expect(doesContextMatchUrl("^/api", "/api")).toBe(true);
    expect(doesContextMatchUrl("^/api", "/api/foo")).toBe(true);
    expect(doesContextMatchUrl("^/api/(.+)", "/api/posts/1")).toBe(true);
    expect(doesContextMatchUrl("^/auth", "/api")).toBe(false);
  });

  it("returns false for invalid regex", () => {
    expect(doesContextMatchUrl("^[", "/api")).toBe(false);
  });
});

describe("ruleMatchesUrl", () => {
  it("matches single context", () => {
    const rule: ProxyRule = {
      context: "/api",
      target: "http://localhost:3000",
    };
    expect(ruleMatchesUrl(rule, "/api/foo")).toBe(true);
    expect(ruleMatchesUrl(rule, "/other")).toBe(false);
  });

  it("matches any of multiple contexts", () => {
    const rule: ProxyRule = {
      context: ["/api", "/placeholder"],
      target: "http://localhost:3000",
    };
    expect(ruleMatchesUrl(rule, "/api/foo")).toBe(true);
    expect(ruleMatchesUrl(rule, "/placeholder/bar")).toBe(true);
    expect(ruleMatchesUrl(rule, "/other")).toBe(false);
  });
});

describe("applyPathRewrite", () => {
  it("returns path when no pathRewrite", () => {
    expect(applyPathRewrite("/api/posts/1", undefined)).toBe("/api/posts/1");
  });

  it("applies function pathRewrite", () => {
    expect(applyPathRewrite("/api/posts/1", (p) => p.replace("/api", ""))).toBe(
      "/posts/1",
    );
  });

  it("applies object pathRewrite (first matching regex)", () => {
    expect(applyPathRewrite("/api/posts/1", { "^/api": "" })).toBe("/posts/1");
    expect(
      applyPathRewrite("/placeholder/foo", { "^/placeholder": "/p" }),
    ).toBe("/p/foo");
  });

  it("applies only first matching rule in object", () => {
    const pathRewrite = { "^/api": "", "^/api/posts": "/posts" };
    // First match is ^/api → "", so result is /posts/1, not /posts/1 from second
    expect(applyPathRewrite("/api/posts/1", pathRewrite)).toBe("/posts/1");
  });

  it("returns original path when no regex matches", () => {
    expect(applyPathRewrite("/other", { "^/api": "" })).toBe("/other");
  });
});

describe("prepareProxyRequest", () => {
  it("rewrites path and preserves query", async () => {
    const app = new Hono();
    let preparedUrl: URL | null = null;
    app.get("/api/posts/1", (c) => {
      const rule: ProxyRule = {
        context: "/api",
        target: "https://jsonplaceholder.typicode.com",
        pathRewrite: { "^/api": "" },
      };
      const prepared = prepareProxyRequest(c, rule);
      preparedUrl = prepared.url;
      expect(prepared.url.pathname).toBe("/posts/1");
      expect(prepared.url.search).toBe("?foo=bar");
      expect(prepared.url.origin).toBe("https://jsonplaceholder.typicode.com");
      return c.text("ok");
    });
    await app.request("http://localhost:3000/api/posts/1?foo=bar");
    expect(preparedUrl).not.toBeNull();
    expect(preparedUrl!.pathname).toBe("/posts/1");
    expect(preparedUrl!.search).toBe("?foo=bar");
  });

  it("sets Host when changeOrigin is not false", async () => {
    const app = new Hono();
    let capturedHeaders: Headers | null = null;
    app.get("/api/foo", (c) => {
      const rule: ProxyRule = {
        context: "/api",
        target: "https://target.example.com",
        changeOrigin: true,
      };
      const prepared = prepareProxyRequest(c, rule);
      capturedHeaders = prepared.headers;
      return c.text("ok");
    });
    await app.request("http://localhost:3000/api/foo");
    expect(capturedHeaders).not.toBeNull();
    expect(capturedHeaders!.get("host")).toBe("target.example.com");
  });

  it("keeps original Host when changeOrigin is false", async () => {
    const app = new Hono();
    let capturedHeaders: Headers | null = null;
    app.get("/api/foo", (c) => {
      const rule: ProxyRule = {
        context: "/api",
        target: "https://target.example.com",
        changeOrigin: false,
      };
      const prepared = prepareProxyRequest(c, rule);
      capturedHeaders = prepared.headers;
      return c.text("ok");
    });
    await app.request("http://localhost:3000/api/foo", {
      headers: { Host: "original.example.com" },
    });
    expect(capturedHeaders).not.toBeNull();
    expect(capturedHeaders!.get("host")).toBe("original.example.com");
  });
});

describe("createHttpProxyMiddleware", () => {
  it("calls next() when no rules", async () => {
    const app = new Hono();
    app.use("*", createHttpProxyMiddleware([]));
    app.get("*", (c) => c.text("static"));
    const res = await app.request("http://localhost:3000/any");
    expect(res.status).toBe(200);
    expect(await res.text()).toBe("static");
  });

  it("calls next() when no rule matches", async () => {
    const app = new Hono();
    const rules: ProxyRule[] = [
      { context: "/api", target: "http://localhost:9999" },
    ];
    app.use("*", createHttpProxyMiddleware(rules));
    app.get("*", (c) => c.text("static"));
    const res = await app.request("http://localhost:3000/other");
    expect(res.status).toBe(200);
    expect(await res.text()).toBe("static");
  });

  it("returns 502 when rule matches but target fails", async () => {
    const app = new Hono();
    const rules: ProxyRule[] = [
      {
        context: "/api",
        target: "http://localhost:99999", // unreachable
      },
    ];
    app.use("*", createHttpProxyMiddleware(rules));
    app.get("*", (c) => c.text("static"));
    const res = await app.request("http://localhost:3000/api/foo");
    expect(res.status).toBe(502);
    expect(await res.text()).toBe("Bad Gateway");
  });
});
