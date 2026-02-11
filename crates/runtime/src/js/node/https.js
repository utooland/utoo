import http from "ext:utoo_rt_ext/node/http";

const https = { ...http };
export default https;
export const { createServer, request, get, Server, STATUS_CODES, METHODS } = http;
