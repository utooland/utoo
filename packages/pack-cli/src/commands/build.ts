import { Command } from "@oclif/core";
import * as utooPack from "@utoo/pack";
import { commonFlags, resolveBuildOptions } from "../utils/common";

export default class Build extends Command {
  static description = "utoopack build";
  static examples = [
    `<%= config.bin %> <%= command.id %> build --project .`,
    `<%= config.bin %> <%= command.id %> build --project . --root ../..`,
    `<%= config.bin %> <%= command.id %> build --webpack`,
  ];
  static flags = commonFlags;

  async run(): Promise<void> {
    const { flags } = await this.parse(Build);
    const { projectOptions, projectPath, rootPath } =
      resolveBuildOptions(flags);
    await utooPack.build(projectOptions, projectPath, rootPath);
  }
}
