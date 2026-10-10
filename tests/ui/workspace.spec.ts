import { test, expect } from "@playwright/test";

test("browse, search, inspect and change views without browser errors", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "studio-assets." }),
  ).toBeVisible();
  await expect(
    page.getByText("Brand guidelines.pdf", { exact: true }),
  ).toBeVisible();
  await page.screenshot({
    path: "artifacts/workspace-preview.png",
    fullPage: true,
  });
  await page.getByLabel("搜索当前目录").fill("guidelines");
  await expect(page.locator("tbody tr")).toHaveCount(1);
  await page.getByLabel("清除搜索").click();
  await page.getByText("Brand assets", { exact: true }).dblclick();
  await expect(page.getByText("Readme.md", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "根目录" }).click();
  await expect(
    page.getByText("Brand guidelines.pdf", { exact: true }),
  ).toBeVisible();
  await page.getByLabel("网格视图").click();
  await expect(page.locator(".object-card")).toHaveCount(9);
  await page.getByLabel("列表视图").click();
  await page.getByLabel("选择 Brand guidelines.pdf", { exact: true }).check();
  await page.getByLabel("对象详情", { exact: true }).click();
  await expect(
    page.locator(".inspector").getByText("application/octet-stream"),
  ).toBeVisible();
  await page.getByLabel("关闭详情").click();
  await page.getByRole("button", { name: /Local development/ }).click();
  await expect(page.locator(".workspace-label")).toContainText(
    "Local development",
  );
  await expect(page.locator("tbody tr")).toHaveCount(9);
  expect(errors).toEqual([]);
});

test("preview refuses real mutations and keeps credentials out of browser storage", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.locator("tbody tr")).toHaveCount(9);
  await page.getByLabel("添加账户", { exact: true }).click();
  const modal = page.getByRole("dialog");
  await expect(
    modal.getByRole("heading", { name: "连接一个新账户" }),
  ).toBeVisible();
  await modal.locator('[name="name"]').fill("New account");
  await modal.locator('[name="accessKey"]').fill("test-access");
  await modal.locator('[name="secretKey"]').fill("test-secret");
  await modal.getByRole("button", { name: "保存账户" }).click();
  await expect(modal.getByRole("alert")).toContainText("当前是界面预览");
  expect(
    await page.evaluate(() =>
      JSON.stringify({
        local: { ...localStorage },
        session: { ...sessionStorage },
      }),
    ),
  ).not.toContain("test-secret");
  await page.keyboard.press("Escape");
  await expect(modal).not.toBeVisible();
  await page.getByLabel("选择 Brand assets", { exact: true }).check();
  await expect(page.getByLabel("删除所选项目")).toBeEnabled();
  await page.getByLabel("删除所选项目").click();
  await expect(modal).toContainText("递归删除 1 个目录及其中所有文件和子目录");
  await modal.getByRole("button", { name: "确认删除" }).click();
  await expect(modal.getByRole("alert")).toContainText("当前是界面预览");
  await expect(page.locator("tbody tr")).toHaveCount(9);
});

test("update preview explains its mode and restores keyboard focus", async ({
  page,
}) => {
  const requests: string[] = [];
  page.on("request", (request) => {
    if (request.url().includes("github.com")) requests.push(request.url());
  });
  await page.goto("/");
  await page.getByLabel("检查应用更新").click();
  const modal = page.getByRole("dialog");
  await expect(modal).toContainText("开发模式与网页预览不检查更新");
  await expect(
    modal.getByRole("button", { name: "检查更新", exact: true }),
  ).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(modal).not.toBeVisible();
  await expect(page.getByLabel("检查应用更新")).toBeFocused();
  expect(requests).toEqual([]);
});

test("minimum desktop width fits the app and dialogs", async ({ page }) => {
  await page.setViewportSize({ width: 960, height: 680 });
  await page.goto("/");
  await expect(page.locator("tbody tr")).toHaveCount(9);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.getByLabel("添加账户", { exact: true }).click();
  await expect(page.getByRole("button", { name: "保存账户" })).toBeInViewport();
});
