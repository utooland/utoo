import { Project, ProjectEndpoint } from "@utoo/web";

const projectInWorker = {
  project: undefined as ProjectEndpoint | undefined,
  async init() {
    projectInWorker.project = Project.fork(new MessageChannel());
  },
  async readFile(path: string, encoding?: "utf8") {
    return await projectInWorker.project!.readFile(path, encoding);
  },
  async writeFile(path: string, content: string) {
    return await projectInWorker.project!.writeFile(path, content);
  },
  async createDirAll(path: string) {
    return await projectInWorker.project!.mkdir(path);
  },
};

setTimeout(async () => {
  await projectInWorker.init();
  console.log(
    "read ./package.json",
    await projectInWorker.readFile("./package.json", "utf8"),
  );
  console.log(
    "createDirAll ./dist",
    await projectInWorker.createDirAll("./dist"),
  );

  console.log(
    "write ./dist/stats.json",
    await projectInWorker.writeFile("./dist/stats.json", JSON.stringify({})),
  );
}, 1000);
