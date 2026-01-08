import { Project as UtooProject } from "@utoo/web";
import { demoFiles } from "../demoFiles";

const projectName = "/utooweb-demo";
export const serviceWorkerScope = "/preview";

// Registry configuration
export const REGISTRIES = {
  npmmirror: "https://registry.npmmirror.com",
  npm: "https://registry.npmjs.org",
} as const;

export type RegistryKey = keyof typeof REGISTRIES | "custom";

export interface UtooConfig {
  registry: RegistryKey;
  customRegistry: string;
  maxConcurrentDownloads: number;
}

const CONFIG_KEY = "utoo-config";

const defaultConfig: UtooConfig = {
  registry: "npmmirror",
  customRegistry: "",
  maxConcurrentDownloads: 20,
};

export const getConfig = (): UtooConfig => {
  try {
    const stored = localStorage.getItem(CONFIG_KEY);
    if (stored) {
      return { ...defaultConfig, ...JSON.parse(stored) };
    }
  } catch {
    // ignore
  }
  return defaultConfig;
};

export const saveConfig = (config: UtooConfig): void => {
  localStorage.setItem(CONFIG_KEY, JSON.stringify(config));
};

export const getRegistryUrl = (config: UtooConfig): string => {
  if (config.registry === "custom") {
    return config.customRegistry || REGISTRIES.npmmirror;
  }
  return REGISTRIES[config.registry] || REGISTRIES.npmmirror;
};

export const hasPackageLock = async (
  project: UtooProject,
): Promise<boolean> => {
  try {
    await project.readFile("package-lock.json", "utf8");
    console.log("hasPackageLock: found");
    return true;
  } catch (e) {
    console.log("hasPackageLock: not found", e);
    return false;
  }
};

export const deleteProjectFiles = async (
  project: UtooProject,
): Promise<void> => {
  try {
    console.log("Deleting node_modules...");
    await project.rmdir("node_modules", { recursive: true });
    console.log("node_modules deleted");
  } catch (e) {
    console.log("Failed to delete node_modules:", e);
  }
  try {
    console.log("Deleting package-lock.json...");
    await project.rm("package-lock.json");
    console.log("package-lock.json deleted");
  } catch (e) {
    console.log("Failed to delete package-lock.json:", e);
  }
};

export const initializeProject = async () => {
  const projectInstance = new UtooProject({
    cwd: projectName,
    workerUrl: "http://localhost:8081/worker.js",
    threadWorkerUrl: "http://localhost:8081/threadWorker.js",
    loaderWorkerUrl: "http://localhost:8081/loaderWorker.js",
    serviceWorker: {
      url: "http://localhost:8081/serviceWorker.js",
      scope: serviceWorkerScope,
      targetDirToCwd: "../utooweb-demo/",
    },
    loadersImportMap: {
      // "xyzLoader": "https://x.y.z"
    },
    logFilter: new URLSearchParams(location.search).get("logFilter") || "",
  });

  // Expose project on window for debugging
  (window as any).project = projectInstance;

  await projectInstance.installServiceWorker();
  await initUtooProject(projectInstance);
  const hasLock = await hasPackageLock(projectInstance);
  console.log("hasLock:", hasLock);
  await installDependencies(projectInstance, hasLock, getConfig());

  return projectInstance;
};

export const updateDependencies = async (
  project: UtooProject,
  config?: UtooConfig,
): Promise<void> => {
  const registryUrl = config ? getRegistryUrl(config) : REGISTRIES.npmmirror;
  const concurrency = config?.maxConcurrentDownloads ?? 20;

  console.log("Updating dependencies (lazy mode)...");

  // Delete node_modules and package-lock.json first
  try {
    await project.rmdir("node_modules", { recursive: true });
    console.log("Deleted node_modules");
  } catch (e) {
    console.log("Failed to delete node_modules:", e);
  }
  try {
    await project.rm("package-lock.json");
    console.log("Deleted package-lock.json");
  } catch (e) {
    console.log("Failed to delete package-lock.json:", e);
  }

  // Regenerate lock file
  console.log(
    "Running deps with registry:",
    registryUrl,
    "concurrency:",
    concurrency,
  );
  const packageLock = await project.deps({
    registry: registryUrl,
    concurrency,
  });
  console.log("deps returned lock file, length:", packageLock.length);

  // Write the lock file to disk
  await project.writeFile("package-lock.json", packageLock, "utf8");

  // Install with lazy extraction
  console.log("Installing...");
  await project.install(packageLock, concurrency);
  console.log("Update complete");
};

export const installDependencies = async (
  project: UtooProject,
  hasLock: boolean = true,
  config?: UtooConfig,
): Promise<void> => {
  const registryUrl = config ? getRegistryUrl(config) : REGISTRIES.npmmirror;
  const concurrency = config?.maxConcurrentDownloads ?? 20;

  console.log(
    "%cOPFS Project:%c Start to install dependencies (lazy mode).",
    "color: blue;",
    "color: green",
  );
  const start = performance.now();

  try {
    let packageLock: string;
    if (hasLock) {
      console.log("Reading existing package-lock.json...");
      packageLock = await project.readFile("package-lock.json", "utf8");
    } else {
      // Check if package.json exists first
      try {
        const pkgJson = await project.readFile("package.json", "utf8");
        console.log("package.json found:", pkgJson.slice(0, 100));
      } catch (e) {
        console.error("package.json not found!", e);
        throw new Error(
          "package.json not found - project may not be initialized",
        );
      }
      // Run deps first to generate package-lock.json
      console.log(
        "Running deps with registry:",
        registryUrl,
        "concurrency:",
        concurrency,
      );
      packageLock = await project.deps({
        registry: registryUrl,
        concurrency,
      });
      console.log("deps returned lock file, length:", packageLock.length);
      // Write the lock file to disk for future use
      await project.writeFile("package-lock.json", packageLock, "utf8");
    }
    console.log("Installing...");
    await project.install(packageLock, concurrency);
  } catch (e) {
    console.error("Failed to install dependencies:", e);
    throw e;
  }
  console.log(
    `%cOPFS Project:%c Finished to install dependencies in ${Math.round(performance.now() - start)} ms.`,
    "color: blue;",
    "color: green",
  );
};

const initUtooProject = async (project: UtooProject): Promise<void> => {
  const createdDirs = new Set<string>();
  for (const filePath in demoFiles) {
    if (Object.prototype.hasOwnProperty.call(demoFiles, filePath)) {
      const content = demoFiles[filePath as keyof typeof demoFiles];
      const lastSlashIndex = filePath.lastIndexOf("/");
      if (lastSlashIndex !== -1) {
        const dirPath = filePath.substring(0, lastSlashIndex);
        if (!createdDirs.has(dirPath)) {
          await project.mkdir(dirPath, { recursive: true });
          createdDirs.add(dirPath);
        }
      }
      await project.writeFile(filePath, content);
    }
  }
};
