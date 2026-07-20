import { initTransport } from "@evjs/client/transport";

initTransport({
  baseUrl: "https://webgw-pre.antgroup-inc.cn/evjsfaas/api/yuyan/demo/v1",
  headers: () => ({
    "x-webgw-appid": "180020010001292147",
    "x-webgw-version": "2.0",
  }),
});
