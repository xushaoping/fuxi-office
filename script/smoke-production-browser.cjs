#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

function playwrightCore() {
  if (process.env.PLAYWRIGHT_CORE_PATH) return require(process.env.PLAYWRIGHT_CORE_PATH);
  try {
    return require("playwright-core");
  } catch (_) {
    const links = path.join(os.homedir(), "Library", "Caches", "ms-playwright", ".links");
    if (fs.existsSync(links)) {
      for (const entry of fs.readdirSync(links)) {
        const modulePath = fs.readFileSync(path.join(links, entry), "utf8").trim();
        if (modulePath && fs.existsSync(path.join(modulePath, "package.json"))) return require(modulePath);
      }
    }
    throw new Error("playwright-core is unavailable; set PLAYWRIGHT_CORE_PATH or install it");
  }
}

async function main() {
  const baseUrl = process.env.FUXI_BROWSER_BASE_URL;
  const tokens = JSON.parse(process.env.FUXI_BROWSER_TOKENS_JSON || "{}");
  const projectName = process.env.FUXI_BROWSER_PROJECT_NAME;
  const roles = ["owner", "modeler", "reviewer", "approver", "viewer"];
  if (!baseUrl || !projectName || !roles.every((role) => tokens[role])) {
    throw new Error("browser smoke requires an isolated base URL, project and five role tokens");
  }

  const { chromium } = playwrightCore();
  const browser = await chromium.launch({ headless: true });
  const result = { status: "passed", browser: "chromium", checks: [], roles: [], pageErrors: [] };
  try {
    const loginContext = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    const loginPage = await loginContext.newPage();
    loginPage.on("pageerror", (error) => result.pageErrors.push(error.message));
    await loginPage.goto(baseUrl + "/login", { waitUntil: "networkidle", timeout: 30000 });
    await loginPage.locator("#login-captcha-code").waitFor({ state: "visible", timeout: 15000 });
    const captcha = loginPage.locator("img.auth-captcha-image");
    await captcha.waitFor({ state: "visible", timeout: 15000 });
    if (!await captcha.evaluate((image) => image.complete && image.naturalWidth > 0)) {
      throw new Error("login captcha image did not render");
    }
    await loginPage.locator("#login-username").fill("browser-smoke-invalid-user");
    await loginPage.locator("#login-password").fill("browser-smoke-invalid-password");
    await loginPage.locator("#login-captcha-code").fill("000");
    const submit = loginPage.getByRole("button", { name: "登 录" });
    await loginPage.waitForFunction(() => {
      const button = document.querySelector('button[type="submit"]');
      return button && !button.disabled;
    }, null, { timeout: 15000 });
    await submit.click();
    await loginPage.locator("#login-error").waitFor({ state: "visible", timeout: 15000 });
    result.checks.push("login_page_captcha_pow_and_rejection");
    await loginContext.close();

    for (const role of roles) {
      const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
      const page = await context.newPage();
      page.on("pageerror", (error) => result.pageErrors.push(`${role}: ${error.message}`));
      await page.addInitScript((token) => localStorage.setItem("auth_token", JSON.stringify(token)), tokens[role]);
      await page.goto(baseUrl + "/projects", { waitUntil: "networkidle", timeout: 30000 });
      try {
        await page.getByRole("heading", { name: "项目管理" }).waitFor({ state: "visible", timeout: 20000 });
      } catch (_) {
        const diagnostic = await page.evaluate(() => ({
          url: location.href,
          tokenPresent: Boolean(localStorage.getItem("auth_token")),
          body: document.body.innerText.slice(0, 1000)
        }));
        throw new Error(`${role} project UI did not render: ${JSON.stringify(diagnostic)}`);
      }
      await page.getByText(projectName, { exact: true }).waitFor({ state: "visible", timeout: 20000 });
      const wasmLoaded = await page.evaluate(() => performance.getEntriesByType("resource")
        .some((entry) => entry.name.includes("fuxi-dmn.wasm")));
      if (!wasmLoaded) throw new Error(`${role} page did not load the release WASM asset`);

      await page.goto(baseUrl + "/", { waitUntil: "networkidle", timeout: 30000 });
      const projectSelector = page.locator("#decision-project-selector");
      await projectSelector.waitFor({ state: "visible", timeout: 20000 });
      await projectSelector.click();
      await page.getByRole("option", { name: projectName, exact: true }).click();
      await page.waitForFunction((name) =>
        document.querySelector("#decision-project-selector")?.innerText.includes(name),
      projectName, { timeout: 15000 });
      const canModel = role === "owner" || role === "modeler";
      const analyzeButton = page.getByRole("button", { name: "开始分析需求", exact: true });
      if (canModel) {
        await analyzeButton.waitFor({ state: "visible", timeout: 15000 });
      } else {
        await analyzeButton.waitFor({ state: "hidden", timeout: 15000 });
      }
      const requirementInput = page.locator("#decision-requirement");
      await requirementInput.waitFor({ state: "visible", timeout: 15000 });
      if (await requirementInput.isDisabled() === canModel) {
        throw new Error(`${role} requirement editor permission did not match its project role`);
      }
      await page.locator("#workspace-tab-validation").click();
      const approvalButton = page.getByRole("button", { name: "批准", exact: true });
      await approvalButton.waitFor({ state: "visible", timeout: 15000 });
      const canReview = role === "reviewer";
      if (await approvalButton.isDisabled() === canReview) {
        throw new Error(`${role} acceptance approval permission did not match its project role`);
      }
      result.roles.push(role);
      if (role === "owner" && process.env.FUXI_BROWSER_SCREENSHOT_DIR) {
        fs.mkdirSync(process.env.FUXI_BROWSER_SCREENSHOT_DIR, { recursive: true });
        await page.screenshot({ path: path.join(process.env.FUXI_BROWSER_SCREENSHOT_DIR,
          "production-browser-projects.png"), fullPage: true });
      }
      await context.close();
    }
    if (result.pageErrors.length) throw new Error("browser reported a JavaScript page error");
    result.checks.push(
      "authenticated_project_management_for_all_roles",
      "release_wasm_loaded",
      "workspace_modeling_controls_match_project_roles",
      "workspace_case_approval_matches_reviewer_role"
    );
    process.stdout.write(JSON.stringify(result) + "\n");
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  process.stderr.write(`browser smoke failed: ${error.message}\n`);
  process.exitCode = 1;
});
