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
