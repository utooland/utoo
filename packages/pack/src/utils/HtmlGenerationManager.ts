import type { EntryOptions, HtmlConfig } from "@utoo/pack-shared";
import path from "path";
import type { NapiWrittenEndpoint } from "../binding";
import type { ConfigComplete } from "../config/types";
import type { Endpoint, RawEntrypoints } from "../core/types";
import { HtmlPlugin } from "../plugins/HtmlPlugin";
import { getInitialAssetsFromEndpointPaths } from "./getInitialAssets";

type ConfigWithGlobalHtml = ConfigComplete & {
  html?: HtmlConfig | HtmlConfig[];
};

interface EndpointHtmlGroup {
  config: HtmlConfig;
  endpoints: Endpoint[];
}

export class HtmlGenerationManager {
  private readonly globalConfigs: HtmlConfig[];
  private readonly appEntries: EntryOptions[];
  private readonly libraryEntries: EntryOptions[];
  private readonly endpointOrder: Endpoint[] = [];
  private readonly endpointGroups = new Map<string, EndpointHtmlGroup>();
  private readonly endpointGroupByEndpoint = new Map<
    Endpoint,
    EndpointHtmlGroup
  >();
  private readonly writtenEndpointPaths = new Map<
    Endpoint,
    NapiWrittenEndpoint
  >();
  private pendingGenerateAll = false;
  private readonly pendingEndpoints = new Set<Endpoint>();
  private generationQueue = Promise.resolve();

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
    this.endpointOrder.length = 0;
    this.endpointGroups.clear();
    this.endpointGroupByEndpoint.clear();
    this.writtenEndpointPaths.clear();
    this.addEntrypoints(
      entrypoints.apps,
      entrypoints.appPaths,
      this.appEntries,
      "app",
    );
    this.addEntrypoints(
      entrypoints.libraries,
      entrypoints.libraryPaths,
      this.libraryEntries,
      "library",
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

    this.pendingGenerateAll = true;
    await this.scheduleGeneration();
  }

  async generateForEndpoint(endpoint: Endpoint) {
    if (!this.enabled) {
      return;
    }

    this.pendingEndpoints.add(endpoint);
    await this.scheduleGeneration();
  }

  private addEntrypoints(
    endpoints: Endpoint[] | undefined,
    paths: NapiWrittenEndpoint[] | undefined,
    entries: EntryOptions[],
    kind: "app" | "library",
  ) {
    if (endpoints && endpoints.length !== entries.length) {
      throw new Error(
        `Expected ${entries.length} ${kind} endpoint(s), received ${endpoints.length}`,
      );
    }
    if (endpoints && paths && paths.length !== endpoints.length) {
      throw new Error(
        `Expected ${endpoints.length} written ${kind} endpoint path(s), received ${paths.length}`,
      );
    }

    endpoints?.forEach((endpoint, index) => {
      this.endpointOrder.push(endpoint);

      const config = entries[index]?.html;
      if (config) {
        const key = this.getHtmlOutputKey(config);
        let group = this.endpointGroups.get(key);
        if (group) {
          // Multiple module scripts extracted from one HTML entry intentionally
          // share an output file. Keep all owning endpoints in the same group so
          // generating that file cannot overwrite all but the last script.
          group.config = config;
          group.endpoints.push(endpoint);
        } else {
          group = { config, endpoints: [endpoint] };
          this.endpointGroups.set(key, group);
        }
        this.endpointGroupByEndpoint.set(endpoint, group);
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

    const assets = getInitialAssetsFromEndpointPaths(
      this.getWrittenEndpointPaths(this.endpointOrder),
    );
    for (const config of this.globalConfigs) {
      await new HtmlPlugin(config).generate(
        this.outputDir,
        assets,
        this.publicPath,
      );
    }
  }

  private async generateEndpointHtml(group: EndpointHtmlGroup) {
    const writtenEndpointPaths = this.getWrittenEndpointPaths(group.endpoints);
    if (writtenEndpointPaths.length === 0) {
      return;
    }

    const assets = getInitialAssetsFromEndpointPaths(writtenEndpointPaths);
    await new HtmlPlugin(group.config).generate(
      this.outputDir,
      assets,
      this.publicPath,
    );
  }

  private getWrittenEndpointPaths(endpoints: Endpoint[]) {
    return endpoints.flatMap((endpoint) => {
      const written = this.writtenEndpointPaths.get(endpoint);
      return written ? [written] : [];
    });
  }

  private getHtmlOutputKey(config: HtmlConfig) {
    const outputDir = path.resolve(config.output?.path ?? this.outputDir);
    return path.join(outputDir, config.filename ?? "index.html");
  }

  private scheduleGeneration() {
    const drain = async () => {
      while (this.pendingGenerateAll || this.pendingEndpoints.size > 0) {
        const generateAll = this.pendingGenerateAll;
        this.pendingGenerateAll = false;
        const endpoints = [...this.pendingEndpoints];
        this.pendingEndpoints.clear();

        await this.generateGlobalHtml();
        const groups = generateAll
          ? [...this.endpointGroups.values()]
          : [
              ...new Set(
                endpoints.flatMap((endpoint) => {
                  const group = this.endpointGroupByEndpoint.get(endpoint);
                  return group ? [group] : [];
                }),
              ),
            ];
        for (const group of groups) {
          await this.generateEndpointHtml(group);
        }
      }
    };
    const queued = this.generationQueue.then(drain, drain);
    this.generationQueue = queued.catch(() => {});
    return queued;
  }
}
