import { test, expect } from "@playwright/test";

test("application concurrency defaults to eight and can be changed", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.locator(".concurrency-badge")).toHaveText("8 路并发");
  await page.getByLabel("传输设置", { exact: true }).click();
  const dialog = page.getByRole("dialog");
  const input = dialog.getByLabel("并发数量");
  await expect(input).toHaveValue("8");
  await input.fill("33");
  await dialog.getByRole("button", { name: "保存设置" }).click();
  await expect(dialog).toBeVisible();
  await expect(page.locator(".concurrency-badge")).toHaveText("8 路并发");
  await input.fill("3");
  await dialog.getByRole("button", { name: "保存设置" }).click();
  await expect(dialog).not.toBeVisible();
  await expect(page.locator(".concurrency-badge")).toHaveText("3 路并发");
  await page.getByLabel("传输设置", { exact: true }).click();
  await expect(page.getByLabel("并发数量")).toHaveValue("3");
});

test("transfer controls resume, pause and cancel with confirmation", async ({
  page,
}) => {
  await page.goto("/?previewTransfers");
  await expect(
    page.getByRole("heading", { name: "studio-assets." }),
  ).toBeVisible();
  await expect(page.locator(".transfer-row")).toHaveCount(2);
  await page.screenshot({
    path: "artifacts/transfers-preview.png",
    fullPage: true,
  });
  const upload = page
    .locator(".transfer-row")
    .filter({ hasText: "Summer collection" });
  const download = page
    .locator(".transfer-row")
    .filter({ hasText: "Photography archive" });
  await upload.getByRole("button", { name: /^继续 / }).click();
  await expect(upload).toHaveAttribute("data-status", "running");
  await download.getByRole("button", { name: /^暂停 / }).click();
  await expect(download).toHaveAttribute("data-status", "paused");
  await upload.getByRole("button", { name: /^取消 / }).click();
  await page.getByRole("button", { name: "保留任务" }).click();
  await expect(upload).toHaveAttribute("data-status", "running");
  await upload.getByRole("button", { name: /^取消 / }).click();
  await page.getByRole("button", { name: "取消并清理" }).click();
  await expect(upload).toHaveAttribute("data-status", "cancelled");
  await page.getByRole("button", { name: "清除已完成及已取消的记录" }).click();
  await expect(page.locator(".transfer-row")).toHaveCount(1);
  await expect(download).toHaveAttribute("data-status", "paused");
});
