import { describe, expect, it } from "vitest";
import { compatOptionsFromWebpack, type WebpackConfig } from "../webpackCompat";

describe("webpack dev server compatibility", () => {
  it.each([true, false, 5])(
    "preserves client reconnect value %s",
    (reconnect) => {
      const options = compatOptionsFromWebpack({
        webpackMode: true,
        entry: "./src/index.ts",
        devServer: { client: { reconnect } },
      } as WebpackConfig);

      expect(options.config.devServer?.client?.reconnect).toBe(reconnect);
    },
  );
});
