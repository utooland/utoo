const ops = Deno.core.ops;

export function hostname() {
  return ops.op_os_hostname();
}

export function platform() {
  return ops.op_os_platform();
}

export function arch() {
  return ops.op_os_arch();
}

export function type_() {
  return ops.op_os_type();
}

export function release() {
  return ops.op_os_release();
}

export function tmpdir() {
  return ops.op_os_tmpdir();
}

export function homedir() {
  return ops.op_os_homedir();
}

export function cpus() {
  return ops.op_os_cpus();
}

export function uptime() {
  return ops.op_os_uptime();
}

export const EOL = platform() === "win32" ? "\r\n" : "\n";

// os.constants - POSIX signals and errno values
// Signal numbers are platform-dependent; use macOS (Darwin) values.
const _signals = {
  SIGHUP: 1,
  SIGINT: 2,
  SIGQUIT: 3,
  SIGILL: 4,
  SIGTRAP: 5,
  SIGABRT: 6,
  SIGEMT: 7,
  SIGFPE: 8,
  SIGKILL: 9,
  SIGBUS: 10,
  SIGSEGV: 11,
  SIGSYS: 12,
  SIGPIPE: 13,
  SIGALRM: 14,
  SIGTERM: 15,
  SIGURG: 16,
  SIGSTOP: 17,
  SIGTSTP: 18,
  SIGCONT: 19,
  SIGCHLD: 20,
  SIGTTIN: 21,
  SIGTTOU: 22,
  SIGIO: 23,
  SIGXCPU: 24,
  SIGXFSZ: 25,
  SIGVTALRM: 26,
  SIGPROF: 27,
  SIGWINCH: 28,
  SIGINFO: 29,
  SIGUSR1: 30,
  SIGUSR2: 31,
};

const _errno = {
  E2BIG: 7,
  EACCES: 13,
  EADDRINUSE: 48,
  EADDRNOTAVAIL: 49,
  EAFNOSUPPORT: 47,
  EAGAIN: 35,
  EALREADY: 37,
  EBADF: 9,
  EBADMSG: 94,
  EBUSY: 16,
  ECANCELED: 89,
  ECHILD: 10,
  ECONNABORTED: 53,
  ECONNREFUSED: 61,
  ECONNRESET: 54,
  EDEADLK: 11,
  EDESTADDRREQ: 39,
  EDOM: 33,
  EDQUOT: 69,
  EEXIST: 17,
  EFAULT: 14,
  EFBIG: 27,
  EHOSTUNREACH: 65,
  EIDRM: 90,
  EILSEQ: 92,
  EINPROGRESS: 36,
  EINTR: 4,
  EINVAL: 22,
  EIO: 5,
  EISCONN: 56,
  EISDIR: 21,
  ELOOP: 62,
  EMFILE: 24,
  EMLINK: 31,
  EMSGSIZE: 40,
  EMULTIHOP: 95,
  ENAMETOOLONG: 63,
  ENETDOWN: 50,
  ENETRESET: 52,
  ENETUNREACH: 51,
  ENFILE: 23,
  ENOBUFS: 55,
  ENODATA: 96,
  ENODEV: 19,
  ENOENT: 2,
  ENOEXEC: 8,
  ENOLCK: 77,
  ENOLINK: 97,
  ENOMEM: 12,
  ENOMSG: 91,
  ENOPROTOOPT: 42,
  ENOSPC: 28,
  ENOSR: 98,
  ENOSTR: 99,
  ENOSYS: 78,
  ENOTCONN: 57,
  ENOTDIR: 20,
  ENOTEMPTY: 66,
  ENOTSOCK: 38,
  ENOTSUP: 45,
  ENOTTY: 25,
  ENXIO: 6,
  EOPNOTSUPP: 102,
  EOVERFLOW: 84,
  EPERM: 1,
  EPIPE: 32,
  EPROTO: 100,
  EPROTONOSUPPORT: 43,
  EPROTOTYPE: 41,
  ERANGE: 34,
  EROFS: 30,
  ESPIPE: 29,
  ESRCH: 3,
  ESTALE: 70,
  ETIME: 101,
  ETIMEDOUT: 60,
  ETXTBSY: 26,
  EWOULDBLOCK: 35,
  EXDEV: 18,
};

export const constants = {
  signals: _signals,
  errno: _errno,
  UV_UDP_REUSEADDR: 4,
};

export function networkInterfaces() {
  return {};
}

export function freemem() {
  return 0;
}

export function totalmem() {
  return 0;
}

export function loadavg() {
  return [0, 0, 0];
}

export function endianness() {
  return "LE";
}

export function userInfo() {
  return { uid: -1, gid: -1, username: "", homedir: homedir(), shell: "" };
}

export default {
  hostname,
  platform,
  arch,
  type: type_,
  release,
  tmpdir,
  homedir,
  cpus,
  uptime,
  EOL,
  constants,
  networkInterfaces,
  freemem,
  totalmem,
  loadavg,
  endianness,
  userInfo,
};
