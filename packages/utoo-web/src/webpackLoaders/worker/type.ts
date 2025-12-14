export interface LoaderRunnerMeta {
  workerData: {
    workerId: number;
    cwd: string;
  };
  loaderAssets: {
    importMaps: Record<string, string>;
    entrypoint: string;
  };
  sab?: SharedArrayBuffer;
}
