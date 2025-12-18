export const SAB_STATE_IDLE = 0;
export const SAB_STATE_REQUEST = 1;
export const SAB_STATE_RESPONSE = 2;
export const SAB_STATE_ERROR = 3;

export const SAB_OP_READ_FILE = 1;
export const SAB_OP_READ_DIR = 2;
export const SAB_OP_WRITE_FILE = 3;
export const SAB_OP_MKDIR = 4;
export const SAB_OP_RM = 5;
export const SAB_OP_RMDIR = 6;
export const SAB_OP_COPY_FILE = 7;
export const SAB_OP_STAT = 8;

// Layout:
// 0: State (Int32)
// 1: Op (Int32)
// 2: Data Length (Int32)
// 12...: Data (Uint8) - Start at byte 12 (3 * 4 bytes)

export class SabComHost {
  private int32: Int32Array;
  private uint8: Uint8Array;

  constructor(private sab: SharedArrayBuffer) {
    this.int32 = new Int32Array(sab);
    this.uint8 = new Uint8Array(sab);
  }

  readRequest() {
    const op = this.int32[1];
    const len = this.int32[2];
    const data = new TextDecoder().decode(this.uint8.slice(12, 12 + len));
    return { op, data };
  }

  writeResponse(data: Uint8Array | string) {
    if (typeof data === "string") {
      data = new TextEncoder().encode(data);
    }
    // TODO: Check size overflow
    this.int32[2] = data.length;
    this.uint8.set(data, 12);
    Atomics.store(this.int32, 0, SAB_STATE_RESPONSE);
    Atomics.notify(this.int32, 0);
  }

  writeError(message: string) {
    const data = new TextEncoder().encode(message);
    this.int32[2] = data.length;
    this.uint8.set(data, 12);
    Atomics.store(this.int32, 0, SAB_STATE_ERROR);
    Atomics.notify(this.int32, 0);
  }
}

export class SabComClient {
  private int32: Int32Array;
  private uint8: Uint8Array;

  constructor(
    private sab: SharedArrayBuffer,
    private notifyHost: () => void,
  ) {
    this.int32 = new Int32Array(sab);
    this.uint8 = new Uint8Array(sab);
  }

  call(op: number, data: string) {
    const encoded = new TextEncoder().encode(data);
    this.int32[1] = op;
    this.int32[2] = encoded.length;
    this.uint8.set(encoded, 12);

    Atomics.store(this.int32, 0, SAB_STATE_REQUEST);
    this.notifyHost();

    Atomics.wait(this.int32, 0, SAB_STATE_REQUEST);

    const state = Atomics.load(this.int32, 0);
    if (state === SAB_STATE_ERROR) {
      const len = this.int32[2];
      const msg = new TextDecoder().decode(this.uint8.slice(12, 12 + len));
      throw new Error(msg);
    }

    const len = this.int32[2];
    return this.uint8.slice(12, 12 + len);
  }
}

export interface SabFileSystem {
  read(path: string): Promise<Uint8Array>;
  readDir(path: string): Promise<any[]>;
  writeString(path: string, content: string): Promise<void>;
  createDirAll(path: string): Promise<void>;
  createDir(path: string): Promise<void>;
  metadata(path: string): Promise<any>;
  removeFile(path: string): Promise<void>;
  removeDir(path: string, recursive: boolean): Promise<void>;
  copyFile(src: string, dst: string): Promise<void>;
}

export const handleSabRequest = async (
  sabHost: SabComHost,
  fs: SabFileSystem,
) => {
  const { op, data: path } = sabHost.readRequest();
  try {
    if (op === SAB_OP_READ_FILE) {
      const bytes = await fs.read(path);
      sabHost.writeResponse(bytes);
    } else if (op === SAB_OP_READ_DIR) {
      const entries = await fs.readDir(path);
      sabHost.writeResponse(JSON.stringify(entries.map((e) => e.toJSON())));
    } else if (op === SAB_OP_WRITE_FILE) {
      const { path: filePath, data: fileContent } = JSON.parse(path);
      // TODO: handle binary content (base64?)
      await fs.writeString(filePath, fileContent);
      sabHost.writeResponse("ok");
    } else if (op === SAB_OP_MKDIR) {
      const { path: dirPath, recursive } = JSON.parse(path);
      if (recursive) {
        await fs.createDirAll(dirPath);
      } else {
        await fs.createDir(dirPath);
      }
      sabHost.writeResponse("ok");
    } else if (op === SAB_OP_RM) {
      const { path: rmPath, recursive } = JSON.parse(path);
      // Mimic internalProject.rm logic
      const metadata = await fs.metadata(rmPath);
      const json = (metadata as any).toJSON
        ? (metadata as any).toJSON()
        : metadata;
      const type = json.type;
      if (type === "file") {
        await fs.removeFile(rmPath);
      } else if (type === "directory") {
        await fs.removeDir(rmPath, !!recursive);
      }
      sabHost.writeResponse("ok");
    } else if (op === SAB_OP_RMDIR) {
      const { path: rmPath, recursive } = JSON.parse(path);
      await fs.removeDir(rmPath, !!recursive);
      sabHost.writeResponse("ok");
    } else if (op === SAB_OP_COPY_FILE) {
      const { src, dst } = JSON.parse(path);
      await fs.copyFile(src, dst);
      sabHost.writeResponse("ok");
    } else if (op === SAB_OP_STAT) {
      const metadata = await fs.metadata(path);
      const json = (metadata as any).toJSON
        ? (metadata as any).toJSON()
        : metadata;
      sabHost.writeResponse(
        JSON.stringify(json, (k, v) =>
          typeof v === "bigint" ? v.toString() : v,
        ),
      );
    } else {
      sabHost.writeError("Unknown op");
    }
  } catch (e: any) {
    sabHost.writeError(e.message);
  }
};
