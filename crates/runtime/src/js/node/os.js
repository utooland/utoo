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
};
