import { Args, Command, Flags } from "@oclif/core";
import * as utooPack from "@utoo/pack";
import fs from "fs";
import path from "path";

export default class Dev extends Command {
  static description = "utoopack dev";
  static examples = [
    `<%= config.bin %> <%= command.id %> dev --project .`,
    `<%= config.bin %> <%= command.id %> dev --project . --root ../..`,
    `<%= config.bin %> <%= command.id %> dev --webpack`,
  ];
  static flags = {
    project: Flags.string({
      char: "p",
      description: "Set the project path",
      required: false,
    }),
    root: Flags.string({
      char: "r",
      description: "Set the root path",
      required: false,
    }),
    webpack: Flags.boolean({
      name: "webpack",
      description: "Enable webpack mode",
      required: false,
    }),
  };

  async run(): Promise<void> {
    const {
      flags: { project, root, webpack },
    } = await this.parse(Dev);

    const cwd = process.cwd();
    let projectPath = path.resolve(cwd, project || cwd);
    let rootPath = root && path.resolve(cwd, root);

    if (webpack) {
      const projectOptions = { webpackMode: true } as utooPack.WebpackConfig;
      await utooPack.build(projectOptions, projectPath, rootPath);
    } else {
      const projectOptions = JSON.parse(
        fs.readFileSync(
          path.resolve(cwd, project || "", "project_options.json"),
          {
            encoding: "utf-8",
          },
        ),
      ) as utooPack.BundleOptions;
      await utooPack.build(projectOptions, projectPath, rootPath);
    }
  }
}
