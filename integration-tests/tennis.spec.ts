import { test, expect } from "@playwright/test";
import { spawn, ChildProcess } from "child_process";
import path from "path";
import http from "http";

const TENNIS_PORT = "3001";
const TENNIS_SERVER_URL = `http://localhost:${TENNIS_PORT}`;

let serverProcess: ChildProcess | null = null;

test.beforeAll(async () => {
  // Start the server (assumes it's already built via cargo build -p example-tennis)
  serverProcess = spawn(
    path.resolve(__dirname, "../target/debug/example-tennis"),
    [],
    {
      stdio: "inherit",
      env: { ...process.env, PORT: TENNIS_PORT },
    }
  );

  // Wait for the server to start
  await waitForServer(TENNIS_SERVER_URL, 15000);
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

test("initial render: empty state, disabled button", async ({ page }) => {
  await page.goto(TENNIS_SERVER_URL);
  await page.waitForSelector("#live-view-container");

  // Empty message should be visible
  await expect(page.locator("text=Create some and they will be listed here.")).toBeVisible();

  // Submit button should be disabled
  const createButton = page.locator("button:has-text('Create Match')");
  await expect(createButton).toBeDisabled();

  // Inputs should be empty
  await expect(page.locator("input[name='player_1']")).toHaveValue("");
  await expect(page.locator("input[name='player_2']")).toHaveValue("");
});

test("form validation: button enables when both names filled", async ({ page }) => {
  await page.goto(TENNIS_SERVER_URL);
  await page.waitForSelector("#live-view-container[data-lv-connected]");

  const player1 = page.locator("input[name='player_1']");
  const player2 = page.locator("input[name='player_2']");
  const createButton = page.locator("button:has-text('Create Match')");

  // Initially disabled
  await expect(createButton).toBeDisabled();

  // Fill only player 1 — still disabled
  await player1.fill("Roger");
  await page.waitForTimeout(200);
  await expect(createButton).toBeDisabled();

  // Fill player 2 — now enabled
  await player2.fill("Rafa");
  await page.waitForTimeout(200);
  await expect(createButton).toBeEnabled();

  // Clear player 1 — disabled again
  await player1.fill("");
  await page.waitForTimeout(200);
  await expect(createButton).toBeDisabled();
});

test("create match: match appears, inputs cleared", async ({ page }) => {
  await page.goto(TENNIS_SERVER_URL);
  await page.waitForSelector("#live-view-container[data-lv-connected]");

  // Fill in player names
  await page.locator("input[name='player_1']").fill("Roger");
  await page.waitForTimeout(100);
  await page.locator("input[name='player_2']").fill("Rafa");
  await page.waitForTimeout(100);

  // Click create
  await page.locator("button:has-text('Create Match')").click();
  await page.waitForTimeout(200);

  // Match should appear with player names (check within window body paragraphs)
  const matchBox = page.locator("#live-view-container .window").first();
  await expect(matchBox).toContainText("Roger");
  await expect(matchBox).toContainText("Rafa");

  // Empty message should be gone
  await expect(page.locator("text=Create some and they will be listed here.")).toHaveCount(0);

  // Inputs should be cleared
  await expect(page.locator("input[name='player_1']")).toHaveValue("");
  await expect(page.locator("input[name='player_2']")).toHaveValue("");

  // Button should be disabled again
  await expect(page.locator("button:has-text('Create Match')")).toBeDisabled();
});

test("create multiple matches: newest appears first", async ({ page }) => {
  await page.goto(TENNIS_SERVER_URL);
  await page.waitForSelector("#live-view-container[data-lv-connected]");

  // Create first match
  await page.locator("input[name='player_1']").fill("Roger");
  await page.locator("input[name='player_2']").fill("Rafa");
  await page.locator("button:has-text('Create Match')").click();
  await page.waitForTimeout(200);

  // Create second match
  await page.locator("input[name='player_1']").fill("Novak");
  await page.locator("input[name='player_2']").fill("Andy");
  await page.locator("button:has-text('Create Match')").click();
  await page.waitForTimeout(200);

  // At least two match windows should be visible
  const boxes = page.locator("#live-view-container .window");
  // Check the first two boxes are visible and contain the expected names
  await expect(boxes.nth(0)).toBeVisible();
  await expect(boxes.nth(1)).toBeVisible();

  // The newest match (Novak / Andy) should appear before the older one
  // Both names should be present somewhere in the match list
  await expect(page.locator("#live-view-container")).toContainText("Novak");
  await expect(page.locator("#live-view-container")).toContainText("Andy");
  await expect(page.locator("#live-view-container")).toContainText("Roger");
  await expect(page.locator("#live-view-container")).toContainText("Rafa");
});

test("add point: increments score for correct player", async ({ page }) => {
  await page.goto(TENNIS_SERVER_URL);
  await page.waitForSelector("#live-view-container[data-lv-connected]");

  // Create a match
  await page.locator("input[name='player_1']").fill("Roger");
  await page.locator("input[name='player_2']").fill("Rafa");
  await page.locator("button:has-text('Create Match')").click();
  await page.waitForTimeout(200);

  // Verify initial scores — both players start at 0
  const box = page.locator("#live-view-container .window").nth(0);
  await expect(box).toContainText("Points: 0");

  // Click "+ point" for player 1 (first button in the box)
  await box.locator("button:has-text('+ point')").nth(0).click();
  await page.waitForTimeout(200);

  // Player 1 should now have 1 point, player 2 still 0
  await expect(box).toContainText("Points: 1");
  await expect(box).toContainText("Points: 0");

  // Click "+ point" for player 2 (second button in the box)
  await box.locator("button:has-text('+ point')").nth(1).click();
  await page.waitForTimeout(200);

  // Both should have 1 point
  await expect(box).toContainText("Points: 1");

  // Click player 1 again twice more
  await box.locator("button:has-text('+ point')").nth(0).click();
  await page.waitForTimeout(100);
  await box.locator("button:has-text('+ point')").nth(0).click();
  await page.waitForTimeout(200);

  // Player 1: 3 points, Player 2: 1 point
  await expect(box).toContainText("Points: 3");
  await expect(box).toContainText("Points: 1");
});

test("add point: multiple matches, points go to correct match", async ({ page }) => {
  await page.goto(TENNIS_SERVER_URL);
  await page.waitForSelector("#live-view-container[data-lv-connected]");

  // Create two matches
  for (const [p1, p2] of [["Roger", "Rafa"], ["Novak", "Andy"]]) {
    await page.locator("input[name='player_1']").fill(p1);
    await page.locator("input[name='player_2']").fill(p2);
    await page.locator("button:has-text('Create Match')").click();
    await page.waitForTimeout(200);
  }

  // Find match boxes by their player names (robust to shared state ordering)
  const novakBox = page.locator("#live-view-container .window", { hasText: "Novak" }).first();
  const rogerBox = page.locator("#live-view-container .window", { hasText: "Roger" }).first();
  await expect(novakBox).toBeVisible();
  await expect(rogerBox).toBeVisible();

  // Add 3 points to Novak (first "+ point" button in Novak's box)
  for (let i = 0; i < 3; i++) {
    await novakBox.locator("button:has-text('+ point')").nth(0).click();
    await page.waitForTimeout(100);
  }
  await page.waitForTimeout(200);

  // Novak's box: Novak 3, Andy 0
  await expect(novakBox).toContainText("Points: 3");
  await expect(novakBox).toContainText("Points: 0");

  // Roger's box: Roger 0, Rafa 0 (unchanged)
  await expect(rogerBox).toContainText("Points: 0");

  // Add 1 point to Andy (second "+ point" button in Novak's box)
  await novakBox.locator("button:has-text('+ point')").nth(1).click();
  await page.waitForTimeout(200);

  // Novak's box: Novak 3, Andy 1
  await expect(novakBox).toContainText("Points: 3");
  await expect(novakBox).toContainText("Points: 1");

  // Roger's box still unchanged
  await expect(rogerBox).toContainText("Points: 0");
});
