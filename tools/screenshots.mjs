// Refreshes the screenshots in `screenshots/` from the browser mock.
//
//   npm run dev            # serves the UI with the simulated backend
//   node tools/screenshots.mjs
//
// Chromium comes from Playwright (`@playwright/test`, a devDependency);
// PLAYWRIGHT_CHROMIUM points at an existing browser when the machine already
// has one, which saves the download.
import { chromium } from "@playwright/test";

const url = process.env.MOCK_URL ?? "http://localhost:1420";
const executablePath = process.env.PLAYWRIGHT_CHROMIUM || undefined;
const browser = await chromium.launch({ executablePath });
// The size the existing screenshots use; keeping it makes diffs readable.
const page = await browser.newPage({ viewport: { width: 1360, height: 860 } });
const shot = async (file) => {
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `screenshots/${file}` });
};

await page.goto(url, { waitUntil: "networkidle" });
await shot("01-library.png");

for (const [tab, file] of [
  ["Downloads", "03-downloads.png"],
  ["Diagnose", "04-diagnostics.png"],
  ["Einstellungen", "06-settings.png"],
]) {
  const button = page.locator(`button:has-text("${tab}")`).first();
  if (await button.count()) {
    await button.click();
    await shot(file);
  } else {
    console.log("no such tab, skipped:", tab);
  }
}

// The game carrying the warning badge: its detail page is the "stuck" shot.
await page.goto(url, { waitUntil: "networkidle" });
await page.waitForTimeout(1200);
await page.locator("text=Quake 3 Arena").first().click();
await shot("02-detail-stuck.png");

await page.goto(`${url}/?wizard`, { waitUntil: "networkidle" });
await shot("07-wizard.png");

await browser.close();
