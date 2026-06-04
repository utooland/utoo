import { describe, expect, it } from "vitest";
import { compatOptionsFromWebpack } from "../webpackCompat";

describe("webpack externals compatibility", () => {
  it("materializes callback-style functional externals", () => {
    const result = compatOptionsFromWebpack(
      {
        webpackMode: true,
        entry: "./src/index.ts",
        externals({ request }, callback) {
          if (request === "react") {
            callback(null, request, "commonjs");
            return;
          }
          callback();
        },
      },
      {
        externalRequests: [
          { request: "react", context: "/project/src", dependencyType: "esm" },
          { request: "lodash", context: "/project/src", dependencyType: "esm" },
        ],
      },
    );

    expect(result.config.externals).toEqual({
      react: "commonjs react",
    });
  });

  it("materializes return-style functional externals", () => {
    const result = compatOptionsFromWebpack(
      {
        webpackMode: true,
        entry: "./src/index.ts",
        externals({ request }) {
          return request === "react" ? true : undefined;
        },
        externalsType: "promise",
      },
      {
        externalRequests: ["react", "lodash"],
      },
    );

    expect(result.config.externals).toEqual({
      react: "promise react",
    });
  });

  it("materializes legacy context/request/callback externals", () => {
    const result = compatOptionsFromWebpack(
      {
        webpackMode: true,
        entry: "./src/index.ts",
        externals(context, request, callback) {
          if (request === "react") {
            callback(null, "React");
            return;
          }
          callback();
        },
      },
      {
        externalRequests: [
          { request: "react", context: "/project/src", dependencyType: "esm" },
        ],
      },
    );

    expect(result.config.externals).toEqual({
      react: "React",
    });
  });

  it("returns an empty externals map when no external requests are available", () => {
    const result = compatOptionsFromWebpack({
      webpackMode: true,
      entry: "./src/index.ts",
      externals({ request }, callback) {
        callback(null, request, "commonjs");
      },
    });

    expect(result.config.externals).toEqual({});
  });
});
