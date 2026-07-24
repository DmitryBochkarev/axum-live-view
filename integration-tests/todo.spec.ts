import { test, expect } from "@playwright/test";
import { spawn, ChildProcess } from "child_process";
import path from "path";
import http from "http";

const TODO_SERVER_URL = `http://localhost:3000`;

let serverProcess: ChildProcess | null = null;

test.beforeAll(async () => {
  // Start the server (assumes it's already built via cargo build -p example-todo)
  serverProcess = spawn(
    path.resolve(__dirname, "../target/debug/example-todo"),
    [],
    {
      stdio: "inherit",
    }
  );

  // Wait for the server to start
  await waitForServer(TODO_SERVER_URL, 15000);
});

test.afterAll(() => {
  if (serverProcess) {
    serverProcess.kill("SIGTERM");
  }
});

async function waitForServer(url: string, timeoutMs: number): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      await new Promise<void>((resolve, reject) => {
        const req = http.get(url, (res) => {
          res.resume();
          resolve();
        });
        req.on("error", reject);
        req.setTimeout(1000, () => {
          req.destroy();
          reject(new Error("timeout"));
        });
      });
      return;
    } catch {
      await new Promise((r) => setTimeout(r, 500));
    }
  }
  throw new Error(`Server at ${url} did not start within ${timeoutMs}ms`);
}

test("filter: all → completed → all shows items again", async ({ page }) => {
  await page.goto(TODO_SERVER_URL);

  // Wait for the live view container to be present
  await page.waitForSelector("#live-view-container");

  // Add two todo items
  await page.fill(".add-todo-input", "First item");
  await page.press(".add-todo-input", "Enter");
  await page.waitForTimeout(100);

  await page.fill(".add-todo-input", "Second item");
  await page.press(".add-todo-input", "Enter");
  await page.waitForTimeout(100);

  // Verify both items are visible
  await expect(page.locator(".todo-text")).toHaveCount(2);
  await expect(page.locator(".todo-text").nth(0)).toHaveText("First item");
  await expect(page.locator(".todo-text").nth(1)).toHaveText("Second item");

  // Click the "Completed" filter button
  await page.click(".filter-btn:nth-child(3)"); // Completed is the 3rd button
  await page.waitForTimeout(100);

  // Verify no items are shown (empty message)
  await expect(page.locator(".todo-list")).toHaveCount(0);
  await expect(page.locator(".empty-msg")).toBeVisible();
  await expect(page.locator(".empty-msg")).toHaveText("No todos to show.");

  // Click the "All" filter button
  await page.click(".filter-btn:nth-child(1)"); // All is the 1st button
  await page.waitForTimeout(100);

  // BUG VERIFICATION: Both items should reappear
  await expect(page.locator(".todo-text")).toHaveCount(2);
  await expect(page.locator(".todo-text").nth(0)).toHaveText("First item");
  await expect(page.locator(".todo-text").nth(1)).toHaveText("Second item");
});

test("filter: add items, click completed, click all — regression test", async ({ page }) => {
  await page.goto(TODO_SERVER_URL);
  await page.waitForSelector("#live-view-container");

  // Add 3 items
  const items = ["Alpha", "Beta", "Gamma"];
  for (const item of items) {
    await page.fill(".add-todo-input", item);
    await page.press(".add-todo-input", "Enter");
    await page.waitForTimeout(50);
  }

  // Verify all 3 items are visible
  await expect(page.locator(".todo-text")).toHaveCount(3);

  // Complete the second item
  await page.locator(".todo-checkbox").nth(1).click();
  await page.waitForTimeout(100);

  // Verify "Clear completed" appears
  await expect(page.locator(".clear-completed")).toBeVisible();

  // Filter to Completed — only Beta should show
  await page.click(".filter-btn:nth-child(3)");
  await page.waitForTimeout(100);
  await expect(page.locator(".todo-text")).toHaveCount(1);
  await expect(page.locator(".todo-text").nth(0)).toHaveText("Beta");

  // Filter to Active — Alpha and Gamma should show
  await page.click(".filter-btn:nth-child(2)");
  await page.waitForTimeout(100);
  await expect(page.locator(".todo-text")).toHaveCount(2);
  await expect(page.locator(".todo-text").nth(0)).toHaveText("Alpha");
  await expect(page.locator(".todo-text").nth(1)).toHaveText("Gamma");

  // Filter back to All — all 3 items should show
  await page.click(".filter-btn:nth-child(1)");
  await page.waitForTimeout(100);
  await expect(page.locator(".todo-text")).toHaveCount(3);
  await expect(page.locator(".todo-text").nth(0)).toHaveText("Alpha");
  await expect(page.locator(".todo-text").nth(1)).toHaveText("Beta");
  await expect(page.locator(".todo-text").nth(2)).toHaveText("Gamma");
});

test("filter: toggle between all filters multiple times", async ({ page }) => {
  await page.goto(TODO_SERVER_URL);
  await page.waitForSelector("#live-view-container");

  // Add one item
  await page.fill(".add-todo-input", "Only item");
  await page.press(".add-todo-input", "Enter");
  await page.waitForTimeout(100);

  // Toggle All → Active → All → Completed → All
  const filterSequence = ["Active", "All", "Completed", "All"];
  for (const filter of filterSequence) {
    if (filter === "All") {
      await page.click(".filter-btn:nth-child(1)");
    } else if (filter === "Active") {
      await page.click(".filter-btn:nth-child(2)");
    } else {
      await page.click(".filter-btn:nth-child(3)");
    }
    await page.waitForTimeout(100);

    if (filter === "Completed") {
      // Item is not completed, so should be hidden
      await expect(page.locator(".empty-msg")).toBeVisible();
    } else {
      // Item should be visible
      await expect(page.locator(".todo-text")).toHaveCount(1);
      await expect(page.locator(".todo-text").nth(0)).toHaveText("Only item");
    }
  }
});
