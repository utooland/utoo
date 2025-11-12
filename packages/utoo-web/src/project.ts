import { handleIssues } from "@utoo/pack-shared";
import * as comlink from "comlink";
import { ForkedProject } from "./forkedProject";
import { installServiceWorker } from "./installServiceWorker";
import { Fork, HandShake } from "./message";
import {
  BuildOutput,
  Dirent,
  PackFile,
  ProjectEndpoint,
  ProjectOptions,
  RawDirent,
  ServiceWorkerOptions,
} from "./type";
import initWasm, { Project as ProjectInternal } from "./utoo";

let ProjectWorker: Worker;

const ConnectedPorts = new Set<MessagePort>();

export class Project implements ProjectEndpoint {
  #mount: Promise<void>;
  #mainThreadWasmInit?: Promise<void>;
  #mainThreadProject?: ProjectInternal;
  #wasmUrl?: string;
  #threadWorkerUrl?: string;

  private serviceWorkerOptions?: ServiceWorkerOptions;

  private remote: comlink.Remote<
    ProjectEndpoint & {
      mount: (
        opt: Omit<ProjectOptions, "workerUrl" | "serviceWorker">,
      ) => Promise<void>;
    }
  >;

  constructor(options: ProjectOptions) {
    const {
      cwd,
      workerUrl,
      wasmUrl,
      threadWorkerUrl,
      serviceWorker,
      logFilter,
    } = options;

    this.#wasmUrl = wasmUrl;
    this.#threadWorkerUrl = threadWorkerUrl;
    this.serviceWorkerOptions = serviceWorker;

    const { port1, port2 } = new MessageChannel();

    this.remote ??= comlink.wrap(port1);

    if (!ProjectWorker) {
      ProjectWorker = workerUrl
        ? new Worker(workerUrl)
        : new Worker(new URL("./worker", import.meta.url));
      window.addEventListener("message", (e) => {
        this.connectWorker(e);
      });

      if (this.serviceWorkerOptions) {
        navigator.serviceWorker.addEventListener("message", (e) => {
          this.connectWorker(e);
        });
      }
    }

    ProjectWorker.postMessage(HandShake, [port2]);

    this.#mount ??= this.remote.mount({
      cwd,
      wasmUrl,
      threadWorkerUrl,
      logFilter,
    });
  }

  private connectWorker(e: MessageEvent) {
    const port = e.ports[0];
    if (e.data === Fork && !ConnectedPorts.has(port)) {
      ProjectWorker.postMessage(HandShake, [port]);
    }
  }

  public async installServiceWorker() {
    if (this.serviceWorkerOptions) {
      const { url, scope } = this.serviceWorkerOptions;
      // Should add "Service-Worker-Allowed": "/" in page root response headers,
      return await installServiceWorker(url, scope);
    }
  }

  public async mount() {
    return await this.#mount;
  }

  get cwd(): string {
    return (this.remote.cwd as any) || "";
  }

  public async install(packageLock: string, maxConcurrentDownloads?: number) {
    await this.#mount;
    return await this.remote.install(packageLock, maxConcurrentDownloads);
  }

  public async build(): Promise<BuildOutput> {
    await this.#mount;
    const res = await this.remote.build();
    handleIssues(res.issues, false, false);
    return res;
  }

  public async readFile(path: string, encoding?: "utf8") {
    await this.#mount;
    return (await this.remote.readFile(path, encoding)) as any;
  }

  public async writeFile(
    path: string,
    content: string | Uint8Array,
    encoding?: "utf8",
  ) {
    await this.#mount;
    return await this.remote.writeFile(path, content, encoding);
  }

  public async copyFile(src: string, dst: string) {
    await this.#mount;
    return await this.remote.copyFile(src, dst);
  }

  public async readdir(
    path: string,
    options?: { recursive?: boolean },
  ): Promise<Dirent[]> {
    await this.#mount;
    const dirEntry = (await this.remote.readdir(
      path,
      options,
    )) as any as RawDirent[];
    return dirEntry.map((e) => new Dirent(e));
  }

  public async mkdir(path: string, options?: { recursive?: boolean }) {
    await this.#mount;
    return await this.remote.mkdir(path, options);
  }

  public async rm(path: string, options?: { recursive?: boolean }) {
    await this.#mount;
    return await this.remote.rm(path, options);
  }

  public async rmdir(path: string, options?: { recursive?: boolean }) {
    await this.#mount;
    return await this.remote.rmdir(path, options);
  }

  public async sigMd5(content: Uint8Array): Promise<string> {
    return await this.remote.sigMd5(content);
  }

  public async gzip(files: PackFile[]): Promise<Uint8Array> {
    await this.#mount;
    console.log("[Worker] Executing gzip via worker/comlink");
    console.time("[Worker] total gzip time (including comlink overhead)");
    const result = await this.remote.gzip(files);
    console.timeEnd("[Worker] total gzip time (including comlink overhead)");
    return result;
  }

  /**
   * Initialize WASM in main thread for direct execution
   */
  private async initMainThreadWasm(): Promise<void> {
    if (!this.#mainThreadWasmInit) {
      this.#mainThreadWasmInit = (async () => {
        await initWasm(this.#wasmUrl);
        // Get cwd from worker
        const cwd = (await this.remote.cwd) as any;
        // Create a Project instance in main thread
        this.#mainThreadProject = new ProjectInternal(
          cwd,
          this.#threadWorkerUrl!,
        );
      })();
    }
    await this.#mainThreadWasmInit;
  }

  /**
   * Execute gzip in main thread without going through worker/comlink
   * Compresses in main thread, then writes via worker to avoid OPFS conflicts
   */
  public async gzipMainThread(files: PackFile[], dest: string): Promise<void> {
    await this.#mount;
    await this.initMainThreadWasm();

    console.log("[Main Thread] Compressing in main thread (pure WASM, no I/O)");
    console.time("[Main Thread] compression time");

    try {
      // Compress in main thread (no file I/O)
      const compressedBytes = this.#mainThreadProject!.gzip(files);
      console.log(
        `[Main Thread] Compressed to ${compressedBytes.length} bytes`,
      );
      console.timeEnd("[Main Thread] compression time");

      // Write to OPFS via worker (to avoid conflicts)
      console.log("[Main Thread] Writing compressed bytes to OPFS via worker");
      await this.writeFile(dest, compressedBytes);
      console.log("[Main Thread] Gzip completed successfully");
    } catch (e) {
      console.error("[Main Thread] Gzip failed with error:", e);
      if (e instanceof Error) {
        console.error("[Main Thread] Error message:", e.message);
        console.error("[Main Thread] Error stack:", e.stack);
      }
      throw e;
    }
  }

  /**
   * Calculate MD5 in main thread directly via WASM
   */
  public async sigMd5MainThread(content: Uint8Array): Promise<string> {
    await this.initMainThreadWasm();

    console.log("[Main Thread] Calculating MD5 directly via WASM");
    console.time("[Main Thread] sigMd5 execution time");

    // Call sigMd5 directly on the main thread Project instance
    const result = this.#mainThreadProject!.sigMd5(content);

    console.timeEnd("[Main Thread] sigMd5 execution time");
    return result;
  }

  public static fork(
    channel: MessageChannel,
    eventSource?: Client | DedicatedWorkerGlobalScope,
  ): ProjectEndpoint {
    (eventSource || (self as DedicatedWorkerGlobalScope)).postMessage(Fork, {
      transfer: [channel.port2],
    });

    return new ForkedProject(channel.port1);
  }
}
