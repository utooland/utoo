import { Command } from "@oclif/core";
import * as utooPack from "@utoo/pack";
import { commonFlags, resolveBuildOptions } from "../utils/common";

export default class Dev extends Command {
  static description = "utoopack dev";
  static examples = [
    `<%= config.bin %> <%= command.id %> dev --project .`,
    `<%= config.bin %> <%= command.id %> dev --project . --root ../..`,
    `<%= config.bin %> <%= command.id %> dev --webpack`,
  ];
  static flags = commonFlags;

  async run(): Promise<void> {
    const { flags } = await this.parse(Dev);
    const { projectOptions, projectPath, rootPath } =
      resolveBuildOptions(flags);
    await utooPack.serve(projectOptions, projectPath, rootPath, {
      logServerInfo: true,
      port: 3000,
    });
  }
}
