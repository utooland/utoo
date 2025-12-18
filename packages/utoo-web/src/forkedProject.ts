import * as comlink from "comlink";
import { PackFile, ProjectEndpoint, RawStats, Stats } from "./type";

export class ForkedProject implements ProjectEndpoint {
  private endpoint: comlink.Remote<ProjectEndpoint>;

  constructor(port: MessagePort) {
    this.endpoint ??= comlink.wrap(port);
  }
  public async install(packageLock: string, maxConcurrentDownloads?: number) {
    return await this.endpoint.install(packageLock, maxConcurrentDownloads);
  }

  public async build() {
    return await this.endpoint.build();
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
