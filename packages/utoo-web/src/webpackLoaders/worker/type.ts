    export interface LoaderRunnerMeta {
      workerData: {
        poolId: string;
        workerId: number;
        cwd: string
      },
     loaderAssets: {
        importMaps: Record<string, string>;
        entrypoint: string;
      }
    }
