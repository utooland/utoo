/**
 * HTTP proxy middleware for Hono dev server.
 * Matches ProxyRule by path, applies pathRewrite, and forwards via Hono proxy().
 * No generic WS proxying in this module.
 */

import type { DevServerProxy, PathRewrite, ProxyRule } from "@utoo/pack-shared";
import type { Context, Next } from "hono";
import { proxy } from "hono/proxy";

/** Path prefix or ^-prefixed regex match (regex tests full path). */
export function doesContextMatchUrl(
  context: string,
  pathOrUrl: string,
): boolean {
  if (context[0] === "^") {
    try {
      return new RegExp(context).test(pathOrUrl);
    } catch {
      return false;
    }
  }
  return pathOrUrl === context || pathOrUrl.startsWith(context + "/");
}

export function ruleMatchesUrl(rule: ProxyRule, pathOrUrl: string): boolean {
  const contexts = Array.isArray(rule.context) ? rule.context : [rule.context];
  return contexts.some((ctx) => doesContextMatchUrl(ctx, pathOrUrl));
}

/**
 * Apply pathRewrite (http-proxy-middleware style).
 * Object: first matching regex → replace, then stop. Function: (path) => new path.
 */
export function applyPathRewrite(
  path: string,
  pathRewrite?: PathRewrite,
): string {
  if (!pathRewrite) return path;

  if (typeof pathRewrite === "function") {
    return pathRewrite(path);
  }

  for (const [pattern, value] of Object.entries(pathRewrite)) {
    try {
      const regex = new RegExp(pattern);
      if (regex.test(path)) {
        return path.replace(regex, value);
      }
    } catch {
      // invalid regex, skip
    }
  }
  return path;
}

export interface PreparedProxyRequest {
  url: URL;
  headers: Headers;
}

/**
 * Build target URL and headers for the proxy request from current context and rule.
 */
export function prepareProxyRequest(
  c: Context,
  rule: ProxyRule,
): PreparedProxyRequest {
  const origUrl = new URL(c.req.url);
  const targetUrl = new URL(rule.target);
  const origPath = c.req.path;
  const rewrittenPath = applyPathRewrite(origPath, rule.pathRewrite);
  targetUrl.pathname = rewrittenPath;
  targetUrl.search = origUrl.search;

  const headers = new Headers(c.req.header());
  if (rule.changeOrigin !== false) {
    headers.set("host", targetUrl.host);
  }

  return { url: targetUrl, headers };
}

/**
 * Hono middleware: on match, proxy to rule.target with pathRewrite and changeOrigin;
 * on error return 502; otherwise next().
 * Caller (e.g. dev.ts) must pass a non-empty array; empty array means no rule matches.
 */
export function createHttpProxyMiddleware(rules: DevServerProxy) {
  return async (c: Context, next: Next) => {
    const rule = rules.find((r) => ruleMatchesUrl(r, c.req.path));
    if (!rule) {
      return next();
    }

    try {
      const prepared = prepareProxyRequest(c, rule);
      const init: RequestInit = {
        method: c.req.method,
        headers: prepared.headers,
      };
      if (c.req.method !== "GET" && c.req.method !== "HEAD") {
        init.body = c.req.raw.body;
      }
      return proxy(prepared.url.toString(), init);
    } catch (err) {
      console.error("[dev proxy] error", c.req.method, c.req.url, err);
      return c.text("Bad Gateway", 502);
    }
  };
}
