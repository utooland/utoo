import { expect, test, type Locator, type Page } from "@playwright/test";

const INSTALL_DONE_PATTERN = /Finished to install dependencies|Update complete/;
const BUILD_DONE_PATTERN = /Pack Project:.*Finished to build|build finished/i;
const FATAL_CONSOLE_PATTERNS = [
  /Worker: Dependency/i,
  /Build failed/i,
  /Failed to install dependencies/i,
  /Installation failed/i,
  /Update failed/i,
  /panic/i,
];

const waitForConsoleMessage = async (
  messages: string[],
  pattern: RegExp,
  startIndex: number,
  timeout: number,
) => {
  await expect
    .poll(
      () => messages.slice(startIndex).some((message) => pattern.test(message)),
      {
        message: `waiting for console message matching ${pattern}`,
        timeout,
      },
    )
    .toBe(true);
};

const expandDirectory = async (directory: Locator) => {
  await expect(directory).toBeVisible();
  if ((await directory.getAttribute("aria-expanded")) !== "true") {
    // The directory row grows action buttons on hover that intentionally stop
    // click propagation. Click the stable disclosure indicator instead.
    await directory.locator(":scope > span").first().click();
  }
  await expect(directory).toHaveAttribute("aria-expanded", "true");
};

const collectConsoleMessages = (page: Page) => {
  const messages: string[] = [];
  page.on("console", (message) => {
    const text = message.text();
    const output = `[browser:${message.type()}] ${text}`;
    messages.push(text);

    if (message.type() === "error") {
      console.error(output);
    } else {
      console.log(output);
    }
  });
  return messages;
};

test("builds and rebuilds the utooweb demo and previews dist/index.html", async ({
  page,
}) => {
  const consoleMessages = collectConsoleMessages(page);
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => {
    pageErrors.push(error.message);
  });

  await page.goto("/");
  await expect(page.getByTestId("utooweb-demo")).toBeVisible();

  const installButton = page.getByTestId("install-dependencies-button");
  await expect(installButton).toBeEnabled({ timeout: 5 * 60 * 1000 });

  const installStartIndex = consoleMessages.length;
  await installButton.click();
  await waitForConsoleMessage(
    consoleMessages,
    INSTALL_DONE_PATTERN,
    installStartIndex,
    5 * 60 * 1000,
  );
  await expect(installButton).toBeEnabled({ timeout: 60 * 1000 });

  const buildButton = page.getByTestId("build-project-button");
  await expect(buildButton).toBeEnabled();

  for (let buildNumber = 0; buildNumber < 2; buildNumber++) {
    const buildStartIndex = consoleMessages.length;
    await buildButton.click();
    await waitForConsoleMessage(
      consoleMessages,
      BUILD_DONE_PATTERN,
      buildStartIndex,
      5 * 60 * 1000,
    );
    await expect(buildButton).toBeEnabled({ timeout: 60 * 1000 });
  }

  await expandDirectory(page.getByTestId("file-tree-directory-root"));
  await expandDirectory(page.getByTestId("file-tree-directory-dist"));

  const indexHtml = page.getByTestId("file-tree-file-dist-index-html");
  await expect(indexHtml).toBeVisible();
  await indexHtml.click();

  await expect(page.getByTestId("preview-iframe")).toBeVisible();
  const preview = page.frameLocator('[data-testid="preview-iframe"]');
  await expect(
    preview.getByText("Advanced Tailwind CSS v3 Examples"),
  ).toBeVisible({ timeout: 2 * 60 * 1000 });
  await expect(preview.getByText("Hello Tailwind v3")).toBeVisible();

  const fatalConsoleMessages = consoleMessages.filter((message) =>
    FATAL_CONSOLE_PATTERNS.some((pattern) => pattern.test(message)),
  );
  expect(fatalConsoleMessages).toEqual([]);
  expect(pageErrors).toEqual([]);
});
