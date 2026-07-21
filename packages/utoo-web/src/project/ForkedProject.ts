import { type ConfigComplete } from "@utoo/pack-shared";
import * as comlink from "comlink";
import {
  DepsOptions,
  HmrSubscribeOptions,
  InstallOptions,
  PackFile,
  ProjectEndpoint,
  RawStats,
  Stats,
  UpdateMessage,
} from "../types";

type RemoteProjectEndpoint = ProjectEndpoint & {
  hmrUnsubscribe: NonNullable<ProjectEndpoint["hmrUnsubscribe"]>;
};

export class ForkedProject implements ProjectEndpoint {
  private endpoint: comlink.Remote<RemoteProjectEndpoint>;

  constructor(port: MessagePort) {
    this.endpoint ??= comlink.wrap(port);
  }

  public async deps(options?: DepsOptions) {
    return await this.endpoint.deps({
      registry: options?.registry ?? undefined,
      concurrency: options?.concurrency ?? undefined,
    });
  }

  public async install(packageLock: string, options?: InstallOptions) {
    return await this.endpoint.install(packageLock, {
      maxConcurrentDownloads: options?.maxConcurrentDownloads,
    });
  }

  public async build(options?: { config?: ConfigComplete; cleanup?: boolean }) {
    return await this.endpoint.build(options);
  }

  public dev(options?: {
    config?: ConfigComplete;
    onUpdate?: (result: any) => void;
  }): void {
    this.endpoint.dev(options);
  }

  public async hmrSubscribe(
    identifier: string,
    callback: (update: unknown) => void,
    options?: HmrSubscribeOptions,
  ): Promise<void> {
    await this.endpoint.hmrSubscribe(
      identifier,
      comlink.proxy(callback),
      options,
    );
  }

  public async hmrUnsubscribe(subscriptionId: string): Promise<void> {
    await this.endpoint.hmrUnsubscribe(subscriptionId);
  }

  public updateInfoSubscribe(
    aggregationMs: number,
    callback: (message: UpdateMessage) => void,
  ): void {
    this.endpoint.updateInfoSubscribe(aggregationMs, comlink.proxy(callback));
  }

  public async readFile(path: string, encoding?: "utf8") {
    return (await this.endpoint.readFile(path, encoding)) as any;
  }

  public async writeFile(
    path: string,
    content: string | Uint8Array,
    encoding?: "utf8",
  ) {
    return await this.endpoint.writeFile(path, content, encoding);
  }

  public async copyFile(src: string, dst: string): Promise<void> {
    return await this.endpoint.copyFile(src, dst);
  }

  public async readdir(path: string, options?: { recursive?: boolean }) {
    return await this.endpoint.readdir(path, options);
  }

  public async mkdir(path: string, options?: { recursive?: boolean }) {
    return await this.endpoint.mkdir(path, options);
  }

  public async rm(path: string, options?: { recursive?: boolean }) {
    return await this.endpoint.rm(path, options);
  }

  public async rmdir(path: string, options?: { recursive?: boolean }) {
    return await this.endpoint.rmdir(path, options);
  }

  public async stat(path: string): Promise<Stats> {
    const raw = (await this.endpoint.stat(path)) as any as RawStats;
    return new Stats(raw);
  }

  public async gzip(files: PackFile[]): Promise<Uint8Array> {
    return await this.endpoint.gzip(files);
  }

  public async sigMd5(content: Uint8Array): Promise<string> {
    return await this.endpoint.sigMd5(content);
  }
}
