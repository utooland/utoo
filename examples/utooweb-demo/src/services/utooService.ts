import { Project as UtooProject } from "@utoo/web";
import { demoFiles } from "../demoFiles";

const projectName = "/utooweb-demo";
export const serviceWorkerScope = "/preview";

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

  await projectInstance.installServiceWorker();
  await initUtooProject(projectInstance);
  await installDependencies(projectInstance);

  return projectInstance;
};

const installDependencies = async (project: UtooProject): Promise<void> => {
  console.log(
    "%cOPFS Project:%c Start to install dependencies.",
    "color: blue;",
    "color: green",
  );
  const start = performance.now();

  const packageLock = await project.readFile("package-lock.json", "utf8");
  try {
    await project.install(packageLock);
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
