import { test, expect } from "@playwright/test";
import { spawn, ChildProcess } from "child_process";
import path from "path";
import http from "http";

const SSE_PORT = "3002";
const SSE_SERVER_URL = `http://localhost:${SSE_PORT}`;

let serverProcess: ChildProcess | null = null;

test.beforeAll(async () => {
  serverProcess = spawn(
    path.resolve(__dirname, "../target/debug/example-counter-sse"),
    [],
    {
      stdio: "inherit",
      env: { ...process.env, PORT: SSE_PORT },
    }
  );

  await waitForServer(SSE_SERVER_URL, 15000);
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

test("SSE: counter increments and decrements via SSE transport", async ({ page }) => {
  await page.goto(SSE_SERVER_URL);

  // Wait for the live view container to appear
  await page.waitForSelector("#live-view-container");

  // Wait for the SSE transport to be fully connected
  await page.waitForSelector("#live-view-container[data-lv-connected]");

  // Verify initial counter value is 0
  await expect(page.locator(".counter-value")).toHaveText("0");

  // Click increment button
  await page.click(".incr-btn");
  await page.waitForTimeout(150);

  // Verify counter changed to 1
  await expect(page.locator(".counter-value")).toHaveText("1");

  // Click increment again
  await page.click(".incr-btn");
  await page.waitForTimeout(150);

  // Verify counter changed to 2
  await expect(page.locator(".counter-value")).toHaveText("2");

  // Click decrement
  await page.click(".decr-btn");
  await page.waitForTimeout(150);

  // Verify counter changed to 1
  await expect(page.locator(".counter-value")).toHaveText("1");

  // Click decrement again
  await page.click(".decr-btn");
  await page.waitForTimeout(150);

  // Verify counter back to 0
  await expect(page.locator(".counter-value")).toHaveText("0");

  // Decrement from 0 should stay at 0
  await page.click(".decr-btn");
  await page.waitForTimeout(150);
  await expect(page.locator(".counter-value")).toHaveText("0");
});

test("SSE: multiple rapid clicks", async ({ page }) => {
  await page.goto(SSE_SERVER_URL);
  await page.waitForSelector("#live-view-container");
  await page.waitForSelector("#live-view-container[data-lv-connected]");

  // Click increment 5 times rapidly
  for (let i = 0; i < 5; i++) {
    await page.click(".incr-btn");
    await page.waitForTimeout(50);
  }

  // Should settle at 5
  await page.waitForTimeout(200);
  await expect(page.locator(".counter-value")).toHaveText("5");

  // Click decrement 3 times
  for (let i = 0; i < 3; i++) {
    await page.click(".decr-btn");
    await page.waitForTimeout(50);
  }

  await page.waitForTimeout(200);
  await expect(page.locator(".counter-value")).toHaveText("2");
});

test("SSE: counter resets on page reload", async ({ page }) => {
  await page.goto(SSE_SERVER_URL);
  await page.waitForSelector("#live-view-container");
  await page.waitForSelector("#live-view-container[data-lv-connected]");

  // Increment to 3
  await page.click(".incr-btn");
  await page.waitForTimeout(100);
  await page.click(".incr-btn");
  await page.waitForTimeout(100);
  await page.click(".incr-btn");
  await page.waitForTimeout(100);
  await expect(page.locator(".counter-value")).toHaveText("3");

  // Reload the page
  await page.reload();
  await page.waitForSelector("#live-view-container");

  // Counter should be back to 0 (new view instance)
  await expect(page.locator(".counter-value")).toHaveText("0");
});
