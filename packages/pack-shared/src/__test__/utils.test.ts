import { describe, expect, it } from "vitest";
import { DevServerProxy, ProxyOptions, ProxyRule } from "../config";
import type { Issue } from "../issue";
import {
  type EntryIssuesMap,
  ModuleBuildError,
  processIssues,
  proxyFromObject,
} from "../utils";

function createIssue(severity: Issue["severity"]): Issue {
  return {
    severity,
    stage: "build",
    filePath: "[project]/src/index.less",
    title: {
      type: "text",
      value: "Unrecognised input",
    },
    description: {
      type: "text",
      value: "Less parser failed",
    },
    documentationLink: "",
    importTraces: [],
  };
}

describe("processIssues", () => {
  it("keeps throwing module build errors for the legacy signature", () => {
    expect(() =>
      processIssues({ issues: [createIssue("error")] }, true, false),
    ).toThrow(ModuleBuildError);
  });

  it("stores entry issues without throwing for the dev-server signature", () => {
    const currentEntryIssues: EntryIssuesMap = new Map();

    processIssues(
      currentEntryIssues,
      "entrypoint:0:client",
      {
        issues: [
          createIssue("error"),
          createIssue("warning"),
          createIssue("info"),
        ],
      },
      false,
      false,
    );

    expect(currentEntryIssues.get("entrypoint:0:client")?.size).toBe(2);

    processIssues(
      currentEntryIssues,
      "entrypoint:0:client",
      { issues: [] },
      false,
      false,
    );

    expect(currentEntryIssues.get("entrypoint:0:client")?.size).toBe(0);
  });
});

describe("proxyFromObject", () => {
  it("converts string targets to ProxyRule with default changeOrigin", () => {
    const input: Record<string, string | ProxyOptions> = {
      "/api": "http://localhost:3000",
      "/auth": "http://localhost:5000",
    };

    const rules: DevServerProxy = proxyFromObject(input);

    expect(rules).toEqual<ProxyRule[]>([
      {
        context: "/api",
        target: "http://localhost:3000",
        changeOrigin: true,
      },
      {
        context: "/auth",
        target: "http://localhost:5000",
        changeOrigin: true,
      },
    ]);
  });

  it("merges object targets and preserves options", () => {
    const input: Record<string, string | ProxyOptions> = {
      "/api": {
        target: "http://localhost:3000",
        pathRewrite: { "^/api": "" },
        secure: false,
      },
    };

    const rules: DevServerProxy = proxyFromObject(input);

    expect(rules).toEqual<ProxyRule[]>([
      {
        context: "/api",
        target: "http://localhost:3000",
        pathRewrite: { "^/api": "" },
        secure: false,
        changeOrigin: true,
      },
    ]);
  });

  it("skips falsy values", () => {
    const input: Record<string, string | ProxyOptions | undefined> = {
      "/api": undefined,
      "/auth": "http://localhost:5000",
    };

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const rules: DevServerProxy = proxyFromObject(input as any);

    expect(rules).toEqual<ProxyRule[]>([
      {
        context: "/auth",
        target: "http://localhost:5000",
        changeOrigin: true,
      },
    ]);
  });

  it("handles multiple context paths (object and string mix)", () => {
    const input: Record<string, string | ProxyOptions> = {
      "/api": "http://localhost:3000",
      "/auth": { target: "http://localhost:5000", changeOrigin: true },
      "/graphql": {
        target: "http://localhost:4000",
        pathRewrite: { "^/graphql": "/gql" },
      },
    };

    const rules: DevServerProxy = proxyFromObject(input);

    expect(rules).toHaveLength(3);
    expect(rules[0]).toMatchObject({
      context: "/api",
      target: "http://localhost:3000",
    });
    expect(rules[1]).toMatchObject({
      context: "/auth",
      target: "http://localhost:5000",
    });
    expect(rules[2]).toMatchObject({
      context: "/graphql",
      target: "http://localhost:4000",
      pathRewrite: { "^/graphql": "/gql" },
    });
  });
});
