import type { UserConfig } from "./config/types";

/**
 * Type-safe config helper for utoopack.config.mjs / utoopack.config.js.
 * Provides type inference and editor support, similar to Vite/Rollup defineConfig.
 *
 * @example
 * ```ts
 * // utoopack.config.mjs
 * import { defineConfig } from '@utoo/pack';
 * export default defineConfig({
 *   entry: [{ import: './src/index.ts', html: { template: './index.html' } }],
 *   output: { path: './dist', clean: true },
 * });
 * ```
 */
export function defineConfig(config: UserConfig): UserConfig {
  return config;
}
