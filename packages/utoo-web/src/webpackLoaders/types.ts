export interface LoaderRunnerMeta {
  workerData: {
    threadId: number;
    cwd: string;
    projectRoot: string;
  };
  loaderAssets: {
    importMaps: Record<string, string>;
    entrypoint: string;
    loadersImportMapFetchCache?: RequestCache;
  };
}
