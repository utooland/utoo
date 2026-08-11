import type { EntryOptions, HtmlConfig } from "@utoo/pack-shared";
import type { NapiWrittenEndpoint } from "../binding";
import type { ConfigComplete } from "../config/types";
import type { Endpoint, RawEntrypoints } from "../core/types";
import { HtmlPlugin } from "../plugins/HtmlPlugin";
import { getInitialAssetsFromEndpointPaths } from "./getInitialAssets";

type ConfigWithGlobalHtml = ConfigComplete & {
  html?: HtmlConfig | HtmlConfig[];
};

export class HtmlGenerationManager {
  private readonly globalConfigs: HtmlConfig[];
  private readonly appEntries: EntryOptions[];
  private readonly libraryEntries: EntryOptions[];
  private readonly endpointConfigs = new Map<Endpoint, HtmlConfig>();
  private readonly writtenEndpointPaths = new Map<
    Endpoint,
    NapiWrittenEndpoint
  >();

  constructor(
    config: ConfigComplete,
    private readonly outputDir: string,
    private readonly publicPath?: string,
  ) {
    const globalHtml = (config as ConfigWithGlobalHtml).html;
    this.globalConfigs = Array.isArray(globalHtml)
      ? globalHtml
      : globalHtml
        ? [globalHtml]
        : [];
    this.appEntries = config.entry.filter((entry) => !entry.library);
    this.libraryEntries = config.entry.filter((entry) => !!entry.library);
  }

  get enabled() {
    return (
      this.globalConfigs.length > 0 ||
      this.appEntries.some((entry) => !!entry.html) ||
      this.libraryEntries.some((entry) => !!entry.html)
    );
  }

  setEntrypoints(entrypoints: RawEntrypoints) {
    this.endpointConfigs.clear();
    this.writtenEndpointPaths.clear();
    this.addEntrypoints(
      entrypoints.apps,
      entrypoints.appPaths,
      this.appEntries,
    );
    this.addEntrypoints(
      entrypoints.libraries,
      entrypoints.libraryPaths,
      this.libraryEntries,
    );
  }

  setWrittenEndpointPath(
    endpoint: Endpoint,
    writtenEndpointPath: NapiWrittenEndpoint,
  ) {
    this.writtenEndpointPaths.set(endpoint, writtenEndpointPath);
  }

  async generateAll() {
    if (!this.enabled) {
      return;
    }

    await this.generateGlobalHtml();
    for (const [endpoint, config] of this.endpointConfigs) {
      await this.generateEndpointHtml(endpoint, config);
    }
  }

  async generateForEndpoint(endpoint: Endpoint) {
    if (!this.enabled) {
      return;
    }

    await this.generateGlobalHtml();
    const config = this.endpointConfigs.get(endpoint);
    if (config) {
      await this.generateEndpointHtml(endpoint, config);
    }
  }

  private addEntrypoints(
    endpoints: Endpoint[] | undefined,
    paths: NapiWrittenEndpoint[] | undefined,
    entries: EntryOptions[],
  ) {
    endpoints?.forEach((endpoint, index) => {
      const config = entries[index]?.html;
      if (config) {
        this.endpointConfigs.set(endpoint, config);
      }

      const writtenEndpointPath = paths?.[index];
      if (writtenEndpointPath) {
        this.writtenEndpointPaths.set(endpoint, writtenEndpointPath);
      }
    });
  }

  private async generateGlobalHtml() {
    if (this.globalConfigs.length === 0) {
      return;
    }

    const assets = getInitialAssetsFromEndpointPaths([
      ...this.writtenEndpointPaths.values(),
    ]);
    for (const config of this.globalConfigs) {
      await new HtmlPlugin(config).generate(
        this.outputDir,
        assets,
        this.publicPath,
      );
    }
  }

  private async generateEndpointHtml(endpoint: Endpoint, config: HtmlConfig) {
    const writtenEndpointPath = this.writtenEndpointPaths.get(endpoint);
    if (!writtenEndpointPath) {
      return;
    }

    const assets = getInitialAssetsFromEndpointPaths([writtenEndpointPath]);
    await new HtmlPlugin(config).generate(
      this.outputDir,
      assets,
      this.publicPath,
    );
  }
}
