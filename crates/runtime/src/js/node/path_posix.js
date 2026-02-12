import path from "ext:utoo_rt_ext/node/path";

const posix = { ...path.posix };
posix.default = posix;

export default posix;
export const { sep, delimiter, join, resolve, dirname, basename, extname, normalize, isAbsolute, relative, parse, format } = path.posix;
