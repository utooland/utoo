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

export const STAT_TYPE_FILE = 0;
export const STAT_TYPE_DIR = 1;

export const SAB_INDEX_STATE = 0;
export const SAB_INDEX_OP = 1;
export const SAB_INDEX_DATA_LEN = 2;
export const SAB_DATA_OFFSET = 12;

// Stat struct layout (starts at byte 12)
// type: Uint8 (1 byte) -> offset 0
// padding: 7 bytes
// size: BigUint64 (8 bytes) -> offset 8
// atimeMs: Float64 (8 bytes) -> offset 16
// mtimeMs: Float64 (8 bytes) -> offset 24
// ctimeMs: Float64 (8 bytes) -> offset 32
// birthtimeMs: Float64 (8 bytes) -> offset 40

const STAT_OFFSET_TYPE = 0;
const STAT_OFFSET_SIZE = 8;
const STAT_OFFSET_ATIME = 16;
const STAT_OFFSET_MTIME = 24;
const STAT_OFFSET_CTIME = 32;
const STAT_OFFSET_BIRTHTIME = 40;

// Layout:
// 0: State (Int32)
// 1: Op (Int32)
// 2: Data Length (Int32)
// 12...: Data (Uint8) - Start at byte 12 (3 * 4 bytes)

export class SabComHost {
  private int32: Int32Array;
  private uint8: Uint8Array;
  private dataView: DataView;

  constructor(private sab: SharedArrayBuffer) {
    this.int32 = new Int32Array(sab);
    this.uint8 = new Uint8Array(sab);
    this.dataView = new DataView(sab);
  }

  readRequest() {
    const op = this.int32[SAB_INDEX_OP];
    const len = this.int32[SAB_INDEX_DATA_LEN];
    const data = new TextDecoder().decode(
      this.uint8.slice(SAB_DATA_OFFSET, SAB_DATA_OFFSET + len),
    );
    return { op, data };
  }

  writeResponse(data: Uint8Array | string) {
    if (typeof data === "string") {
      data = new TextEncoder().encode(data);
    }
    // TODO: Check size overflow
    this.int32[SAB_INDEX_DATA_LEN] = data.length;
    this.uint8.set(data, SAB_DATA_OFFSET);
    Atomics.store(this.int32, SAB_INDEX_STATE, SAB_STATE_RESPONSE);
    Atomics.notify(this.int32, SAB_INDEX_STATE);
  }

  writeError(message: string) {
    const data = new TextEncoder().encode(message);
    this.int32[SAB_INDEX_DATA_LEN] = data.length;
    this.uint8.set(data, SAB_DATA_OFFSET);
    Atomics.store(this.int32, SAB_INDEX_STATE, SAB_STATE_ERROR);
    Atomics.notify(this.int32, SAB_INDEX_STATE);
  }

  writeStat(
    type: number,
    size: bigint,
    atimeMs: number,
    mtimeMs: number,
    ctimeMs: number,
    birthtimeMs: number,
  ) {
    const base = SAB_DATA_OFFSET;
    this.dataView.setUint8(base + STAT_OFFSET_TYPE, type);
    this.dataView.setBigUint64(base + STAT_OFFSET_SIZE, size, true);
    this.dataView.setFloat64(base + STAT_OFFSET_ATIME, atimeMs, true);
    this.dataView.setFloat64(base + STAT_OFFSET_MTIME, mtimeMs, true);
    this.dataView.setFloat64(base + STAT_OFFSET_CTIME, ctimeMs, true);
    this.dataView.setFloat64(base + STAT_OFFSET_BIRTHTIME, birthtimeMs, true);

    Atomics.store(this.int32, SAB_INDEX_STATE, SAB_STATE_RESPONSE);
    Atomics.notify(this.int32, SAB_INDEX_STATE);
  }
}

export class SabComClient {
  private int32: Int32Array;
  private uint8: Uint8Array;
  private dataView: DataView;

  constructor(
    private sab: SharedArrayBuffer,
    private notifyHost: () => void,
  ) {
    this.int32 = new Int32Array(sab);
    this.uint8 = new Uint8Array(sab);
    this.dataView = new DataView(sab);
  }

  call(op: number, data: string) {
    const encoded = new TextEncoder().encode(data);
    this.int32[SAB_INDEX_OP] = op;
    this.int32[SAB_INDEX_DATA_LEN] = encoded.length;
    this.uint8.set(encoded, SAB_DATA_OFFSET);

    Atomics.store(this.int32, SAB_INDEX_STATE, SAB_STATE_REQUEST);
    this.notifyHost();

    Atomics.wait(this.int32, SAB_INDEX_STATE, SAB_STATE_REQUEST);

    const state = Atomics.load(this.int32, SAB_INDEX_STATE);
    if (state === SAB_STATE_ERROR) {
      const len = this.int32[SAB_INDEX_DATA_LEN];
      const msg = new TextDecoder().decode(
        this.uint8.slice(SAB_DATA_OFFSET, SAB_DATA_OFFSET + len),
      );
      throw new Error(msg);
    }

    const len = this.int32[SAB_INDEX_DATA_LEN];
    return this.uint8.slice(SAB_DATA_OFFSET, SAB_DATA_OFFSET + len);
  }

  callStat(path: string) {
    const encoded = new TextEncoder().encode(path);
    this.int32[SAB_INDEX_OP] = SAB_OP_STAT;
    this.int32[SAB_INDEX_DATA_LEN] = encoded.length;
    this.uint8.set(encoded, SAB_DATA_OFFSET);

    Atomics.store(this.int32, SAB_INDEX_STATE, SAB_STATE_REQUEST);
    this.notifyHost();

    Atomics.wait(this.int32, SAB_INDEX_STATE, SAB_STATE_REQUEST);

    const state = Atomics.load(this.int32, SAB_INDEX_STATE);
    if (state === SAB_STATE_ERROR) {
      const len = this.int32[SAB_INDEX_DATA_LEN];
      const msg = new TextDecoder().decode(
        this.uint8.slice(SAB_DATA_OFFSET, SAB_DATA_OFFSET + len),
      );
      throw new Error(msg);
    }

    const base = SAB_DATA_OFFSET;
    return {
      type: this.dataView.getUint8(base + STAT_OFFSET_TYPE),
      size: this.dataView.getBigUint64(base + STAT_OFFSET_SIZE, true),
      atimeMs: this.dataView.getFloat64(base + STAT_OFFSET_ATIME, true),
      mtimeMs: this.dataView.getFloat64(base + STAT_OFFSET_MTIME, true),
      ctimeMs: this.dataView.getFloat64(base + STAT_OFFSET_CTIME, true),
      birthtimeMs: this.dataView.getFloat64(base + STAT_OFFSET_BIRTHTIME, true),
    };
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

      const type = json.type === "directory" ? STAT_TYPE_DIR : STAT_TYPE_FILE;
      const size = BigInt(json.file_size || 0);
      const atimeMs = Number(json.atimeMs || 0);
      const mtimeMs = Number(json.mtimeMs || 0);
      const ctimeMs = Number(json.ctimeMs || 0);
      const birthtimeMs = Number(json.birthtimeMs || 0);

      sabHost.writeStat(type, size, atimeMs, mtimeMs, ctimeMs, birthtimeMs);
    } else {
      sabHost.writeError("Unknown op");
    }
  } catch (e: any) {
    sabHost.writeError(e.message);
  }
};
