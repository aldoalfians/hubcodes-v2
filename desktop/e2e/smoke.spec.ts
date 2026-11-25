import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";

/** `get_config` as the Rust side really sends it (sparse: empty fields left out). Regenerate: HUBCODES_DUMP_CONFIG=e2e/fixtures/config-from-rust.json cargo test dump (in src-tauri), then reset selfName and path. */
const rustConfig = readFileSync(new URL("./fixtures/config-from-rust.json", import.meta.url), "utf8");

/** Collects page errors so every test can assert the UI rendered cleanly. */
function watchErrors(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => { if (m.type() === "error" && !m.text().startsWith("Failed to load resource")) errors.push(m.text()); });
  // A missing favicon is not a UI error; any other failed request is.
  page.on("response", (r) => { if (r.status() >= 400 && !r.url().endsWith("/favicon.ico")) errors.push(`${r.status()} ${r.url()}`); });
  return errors;
}

async function open(page: Page) {
  await page.goto("/");
  await expect(page.locator(".node").first()).toBeVisible();
}

test("canvas shows the agents as cards and in the rail", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  const cards = await page.locator(".node").count();
  expect(cards).toBeGreaterThan(1);
  await expect(page.locator(".rail .rail-item")).toHaveCount(cards - (await page.locator(".note-node").count()));
  await expect(page.locator(".ui-error")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("every settings page renders", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  const nav = page.getByRole("navigation", { name: "Settings sections" });
  await expect(nav).toBeVisible();
  await expect(page.getByRole("dialog", { name: "Settings" }).getByLabel("App version")).toContainText(/hubcodes \d+\.\d+\.\d+/);
  const items = nav.getByRole("menuitem");
  const n = await items.count();
  expect(n).toBeGreaterThanOrEqual(15);
  for (let i = 0; i < n; i++) {
    const item = items.nth(i);
    const label = (await item.innerText()).trim();
    await item.click();
    await expect(item, label).toHaveClass(/ant-menu-item-selected/);
    await expect(page.locator(".settings-page"), label).not.toBeEmpty();
    await expect(page.locator(".ui-error"), label).toHaveCount(0);
  }
  expect(errors).toEqual([]);
});

test("board columns fit the dialog and a task can be added", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("navigation", { name: "Views" }).getByText("Board").click();
  const cols = page.locator(".board-dialog .board-cols");
  await expect(cols).toBeVisible();
  const fit = await cols.evaluate((el) => el.scrollWidth <= el.clientWidth + 1);
  expect(fit, "board columns overflow the dialog").toBe(true);
  const before = await page.locator(".board-dialog .task").count();
  await page.getByLabel("New task").fill("Smoke test task");
  await page.getByLabel("New task").press("Enter");
  await expect(page.locator(".board-dialog .task")).toHaveCount(before + 1);
  await expect(page.getByText("Smoke test task")).toBeVisible();
  expect(errors).toEqual([]);
});

test("picking a theme repaints the app", async ({ page }) => {
  await open(page);
  const bg = () => page.evaluate(() => document.documentElement.style.getPropertyValue("--bg").trim());
  const first = await bg();
  await page.getByRole("button", { name: "Settings" }).click();
  const cards = page.locator(".theme-card");
  await expect(cards.first()).toBeVisible();
  const index = await cards.evaluateAll((els) => els.findIndex((e) => !e.classList.contains("is-on")));
  const other = cards.nth(index);
  await other.click();
  await expect(other).toHaveClass(/is-on/);
  expect(await bg()).not.toBe(first);
});

test("a room takes a post and a new room can be made", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("navigation", { name: "Views" }).getByText("Rooms").click();
  const rooms = page.getByRole("navigation", { name: "Rooms" });
  await expect(rooms.getByRole("button", { name: /auth/ })).toBeVisible();
  await page.getByLabel("Post to auth").fill("Smoke test post");
  await page.getByLabel("Post to auth").press("Enter");
  await expect(page.locator(".rm-log")).toContainText("Smoke test post");
  await rooms.getByRole("button", { name: "New room" }).click();
  await page.getByLabel("New room name").fill("Smoke Room");
  await expect(page.getByLabel("New room name")).toHaveValue("smoke-room");
  await page.getByRole("button", { name: "Create room" }).click();
  await expect(page.getByRole("region", { name: "Room smoke-room" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("the inbox puts the keys on the buttons they press", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Inbox" }).click();
  const first = page.locator(".ib-card").first();
  await expect(first.getByRole("button", { name: /Approve/ })).toContainText("y");
  await expect(page.getByText("via the shared page")).toBeVisible();
  expect(errors).toEqual([]);
});

test("every settings page renders with the config as Rust sends it", async ({ page }) => {
  const errors = watchErrors(page);
  await page.addInitScript((json) => { (globalThis as { __hubcodesConfig?: unknown }).__hubcodesConfig = JSON.parse(json); }, rustConfig);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  const items = page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem");
  const n = await items.count();
  for (let i = 0; i < n; i++) {
    const item = items.nth(i);
    const label = (await item.innerText()).trim();
    await item.click();
    await expect(page.locator(".settings-page"), label).not.toBeEmpty();
    await expect(page.locator(".ui-error"), label).toHaveCount(0);
  }
  await page.getByRole("menuitem", { name: "Policy" }).click();
  await expect(page.getByRole("list", { name: "Tools that never ask" }).getByRole("listitem").filter({ hasText: "Read" })).toBeVisible();
  expect(errors).toEqual([]);
});

const paneWrites = (page: Page) => page.evaluate(() => ((globalThis as { __paneWrites?: string[] }).__paneWrites ?? []).splice(0));
const cardTerm = (page: Page, name: string) =>
  page.locator(".react-flow__node").filter({ has: page.locator(".node-name", { hasText: new RegExp(`^${name}$`) }) }).locator(".pane-box .xterm");

test("a local agent's card has no chat box; another device is a rail badge until shown, then its card keeps the box", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await expect(page.getByRole("textbox", { name: "Instruct builder" })).toHaveCount(0);
  await expect(page.getByRole("textbox", { name: "Instruct planner" })).toHaveCount(0);
  // Another device's agents are a badge in the rail, not cards, until asked for.
  await expect(page.getByRole("textbox", { name: "Instruct mini/tester" })).toHaveCount(0);
  const badge = page.getByRole("button", { name: /^mini: 1 agent/ });
  await expect(badge).toBeVisible();
  await badge.click();
  await expect(page.locator(".device-pop").getByText("tester", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Show on this canvas" }).click();
  // mini/tester runs on another device: no live terminal here, so it keeps the box.
  await expect(page.getByRole("textbox", { name: "Instruct mini/tester" })).toBeVisible();
  await page.keyboard.press("Escape");
  await cardTerm(page, "builder").click();
  await paneWrites(page);
  await page.keyboard.type("hi");
  await page.keyboard.press("Enter");
  expect((await paneWrites(page)).join("|")).toBe("%2:h|%2:i|%2:\r");
  expect(errors).toEqual([]);
});

test("the open terminal says whether the agent checks with its lead, and a removed line ends that", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.locator(".react-flow__node").filter({ has: page.locator(".node-name", { hasText: /^reviewer$/ }) }).dblclick({ position: { x: 40, y: 12 } });
  // reviewer reaches planner through builder.
  await expect(page.locator(".sheet-foot")).toContainText("reviewer, which checks with planner");
  await page.keyboard.press("Escape");
  const label = page.locator(".edge-label", { hasText: "task" });
  await label.click();
  await label.getByRole("button", { name: "Remove" }).click();
  await page.locator(".react-flow__node").filter({ has: page.locator(".node-name", { hasText: /^reviewer$/ }) }).dblclick({ position: { x: 40, y: 12 } });
  await expect(page.locator(".sheet-foot")).toContainText("Typing goes straight to the agent.");
  expect(errors).toEqual([]);
});

test("an agent switches provider or model from its panel and keeps its role", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.locator(".react-flow__node").filter({ has: page.locator(".node-name", { hasText: /^planner$/ }) }).dblclick({ position: { x: 40, y: 12 } });
  await page.locator(".sheet-head").getByRole("button", { name: "Switch" }).click();
  let dialog = page.getByRole("dialog", { name: "Switch planner" });
  await expect(dialog).toContainText("the orchestrator role");
  await expect(dialog.getByRole("button", { name: "Switch" })).toBeDisabled();
  // Same provider, another model: the conversation goes on.
  await dialog.getByRole("combobox", { name: "Model" }).fill("sonnet");
  await page.keyboard.press("Enter");
  await expect(dialog).toContainText("The conversation continues");
  await dialog.getByRole("button", { name: "Switch" }).click();
  await expect(page.getByText(/planner now runs on claude · sonnet: the conversation continues/)).toBeVisible();
  await expect(page.locator(".sheet-head")).toContainText("claude · sonnet");
  // Another provider: a hand-over note.
  await page.locator(".sheet-head").getByRole("button", { name: "Switch" }).click();
  dialog = page.getByRole("dialog", { name: "Switch planner" });
  await dialog.getByRole("combobox", { name: "Provider" }).click();
  await page.locator(".ant-select-dropdown:visible .ant-select-item-option").filter({ hasText: "codex" }).click();
  await expect(dialog).toContainText("A new tool starts with a hand-over note");
  await dialog.getByRole("button", { name: "Switch" }).click();
  await expect(page.getByText(/planner now runs on codex: it started with a hand-over note/)).toBeVisible();
  await expect(page.locator(".sheet-head")).toContainText("orchestrator");
  await expect(page.locator(".sheet-head")).toContainText("codex");
  expect(errors).toEqual([]);
});

test("a card's toolbar switches the agent without opening its panel", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.locator(".react-flow__node").filter({ has: page.locator(".node-name", { hasText: /^reviewer$/ }) }).click({ position: { x: 40, y: 12 } });
  await page.locator(".node-tools").getByRole("button", { name: "Switch" }).click();
  const dialog = page.getByRole("dialog", { name: "Switch reviewer" });
  await dialog.getByRole("combobox", { name: "Model" }).fill("sonnet");
  await page.keyboard.press("Enter");
  await dialog.getByRole("button", { name: "Switch" }).click();
  await expect(page.getByText(/reviewer now runs on \w+ · sonnet/)).toBeVisible();
  // The card stays while its window is replaced, under a spinner.
  await expect(page.locator(".react-flow__node").filter({ has: page.locator(".node-name", { hasText: /^reviewer$/ }) })).toContainText(/Switching to \w+ · sonnet/);
  await expect(page.locator(".sheet-head")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("New agent reads the provider's models on this device again", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: /New agent/ }).first().click();
  const dialog = page.getByRole("dialog", { name: "New agent" });
  await expect(dialog).toContainText(/\d+ from /);
  await dialog.getByRole("button", { name: "Sync models" }).click();
  await expect(dialog.getByRole("button", { name: "Sync models" })).toBeEnabled();
  await expect(dialog).toContainText(/\d+ from /);
  expect(errors).toEqual([]);
});

test("Hermes is a provider, and New agent opens its sign-in in a pane", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: /New agent/ }).first().click();
  const dialog = page.getByRole("dialog", { name: "New agent" });
  await dialog.getByText("hermes", { exact: false }).first().click();
  await expect(dialog).toContainText("hermes model");
  await dialog.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText(/hermes-login is open on the canvas/)).toBeVisible();
  expect(errors).toEqual([]);
});

test("auto team limits are set per role in Settings", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /^Agents/ }).click();
  const group = page.locator(".st-group").filter({ hasText: "Auto team" });
  await group.getByRole("switch", { name: "Add agents when work piles up" }).click();
  await expect(group.getByRole("spinbutton", { name: "Extra agents per role" })).toHaveValue("2");
  await expect(group.getByRole("spinbutton", { name: "Extra orchestrator agents" })).toHaveValue("0");
  await group.getByRole("spinbutton", { name: "Extra reviewer agents" }).fill("3");
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByText(/^Saved/)).toBeVisible();
  expect(errors).toEqual([]);
});

test("a server that signs in with a browser signs in from Settings without leaving it", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  const nav = page.getByRole("navigation", { name: "Settings sections" });
  await nav.getByRole("menuitem", { name: /Memory/ }).click();
  await page.getByRole("radio", { name: /MCP server/ }).click();
  await page.getByRole("textbox", { name: "Server URL" }).fill("https://mem.acme.test/mcp/");
  await page.getByText("Browser (OAuth)").click();
  await expect(page.getByRole("textbox", { name: "Server URL" })).toHaveValue("https://mem.acme.test/mcp/");
  // Signing in reaches the server (the browser opens when needed) and stays in Settings.
  await page.getByRole("button", { name: "Sign in now" }).click();
  await expect(page.getByText(/Signed in · \d+ tools? on the server/)).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Settings sections" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("providers list the models each installed tool offers on this device", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /Providers/ }).click();
  const list = page.getByRole("list", { name: "Models per provider" });
  await expect(list).toBeVisible();
  await expect(list.getByRole("listitem").filter({ hasText: /^.?claude\d/ })).toContainText(/\d+ models?/);
  await page.getByRole("button", { name: "Read again" }).click();
  await expect(list.getByRole("listitem").first()).not.toContainText("reading…");
  expect(errors).toEqual([]);
});

test("Devices walks through pairing: Remote Login, a key to copy, the line for the other device", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /Devices/ }).click();
  const pair = page.locator(".st-group").filter({ hasText: "Pair with another device" });
  await expect(pair).toBeVisible();
  // A device is paired already: the steps are folded until asked for.
  await expect(pair).toContainText("Paired with 1 device");
  await pair.getByRole("button", { name: "Pair another device" }).click();
  await expect(pair.locator(".pair-step.is-todo")).toHaveCount(2);
  await expect(pair.getByRole("button", { name: "Download" })).toBeVisible();
  await expect(pair.getByRole("button", { name: /Copy brew install --cask tailscale/ })).toBeVisible();
  await expect(pair.getByRole("button", { name: "Open Sharing" })).toBeVisible();
  // No key yet: create one, then the copy button replaces it.
  await pair.getByRole("button", { name: "Create key" }).click();
  await expect(pair.getByRole("button", { name: "Copy command" })).toBeVisible();
  await expect(pair.getByText(/hubcodes peer add \S+ macbook --user demo/)).toBeVisible();
  expect(errors).toEqual([]);
});

test("a tracker another device already polls is flagged, in the list and while editing", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /Trackers/ }).click();
  await expect(page.getByText("Also polled on mini")).toBeVisible();
  await page.getByText("jira", { exact: true }).first().click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("mini already polls these issues")).toBeVisible();
  // A different query reads different issues: the warning goes.
  await dialog.getByRole("combobox", { name: "Project" }).click();
  await page.locator(".ant-select-item-option").filter({ hasText: "PMP · Procure to pay" }).click();
  await expect(dialog.getByText("mini already polls these issues")).toBeHidden();
  expect(errors).toEqual([]);
});

test("the message dock sends a person's message with files, to a local agent and to one on another device", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  const dock = page.getByRole("region", { name: "Message agents" });
  await expect(dock).toBeVisible();
  // ⌘L focuses it.
  await page.locator(".react-flow__pane").click({ position: { x: 12, y: 12 } });
  await page.keyboard.press("Meta+l");
  await expect(dock.getByRole("textbox", { name: "Message" })).toBeFocused();
  // Pick a local agent, attach a screenshot, send.
  const to = dock.getByRole("combobox", { name: "Send to" });
  await to.click();
  await page.locator(".ant-select-item-option").filter({ hasText: /^.?builder$/ }).first().click();
  await page.keyboard.press("Escape");
  await dock.locator('input[type="file"]').setInputFiles({ name: "Screen Shot.png", mimeType: "image/png", buffer: Buffer.from("89504e470d0a1a0a", "hex") });
  await expect(dock.getByRole("list", { name: "Attachments" }).getByText("Screen Shot.png")).toBeVisible();
  await dock.getByRole("textbox", { name: "Message" }).fill("why does the login button overlap here?");
  await expect(dock.getByRole("button", { name: "Send" })).toBeEnabled();
  await dock.getByRole("textbox", { name: "Message" }).press("Enter");
  await expect(dock.getByText(/Sent to .*builder.* with 1 file/)).toBeVisible();
  await expect(dock.getByRole("list", { name: "Attachments" })).toHaveCount(0);
  await expect(dock.getByRole("textbox", { name: "Message" })).toHaveValue("");
  // An agent on another device: the file is copied there first.
  await to.click();
  await page.locator(".ant-select-item-option").filter({ hasText: "tester" }).first().click();
  await page.keyboard.press("Escape");
  await dock.locator('input[type="file"]').setInputFiles({ name: "e2e.log", mimeType: "text/plain", buffer: Buffer.from("3 failed") });
  await dock.getByRole("button", { name: "Send" }).click();
  await expect(dock.getByText(/Sent to .*mini\/tester.* with 1 file/)).toBeVisible();
  // Hide it; the pill keeps a draft and brings it back, and so does ⌘L.
  await dock.getByRole("textbox", { name: "Message" }).fill("half written");
  await dock.getByRole("button", { name: "Hide the message dock" }).click();
  await expect(dock).toBeHidden();
  const pill = page.getByRole("button", { name: "Show the message dock" });
  await expect(pill).toContainText("draft");
  await pill.click();
  await expect(dock.getByRole("textbox", { name: "Message" })).toHaveValue("half written");
  await dock.getByRole("button", { name: "Hide the message dock" }).click();
  await page.locator(".react-flow__pane").click({ position: { x: 12, y: 12 } });
  await page.keyboard.press("Meta+l");
  await expect(dock.getByRole("textbox", { name: "Message" })).toBeFocused();
  // Hidden stays hidden after a reload.
  await dock.getByRole("button", { name: "Hide the message dock" }).click();
  await page.reload();
  await expect(page.getByRole("button", { name: "Show the message dock" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("an Atlassian site twg is signed in to is added with one click", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /Accounts/ }).click();
  await expect(page.locator(".st-notice")).toContainText("twg is signed in to demo.atlassian.net, which has no account here yet.");
  await page.getByRole("button", { name: "Add from twg" }).click();
  await expect(page.getByText(/demo\.atlassian\.net · twg sign-in/)).toBeVisible();
  await expect(page.getByText("Signed in as Demo Person")).toBeVisible();
  await expect(page.getByRole("button", { name: "Add from twg" })).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("connecting an account once: trackers, sources and triggers are picked, not typed", async ({ page }) => {
  test.setTimeout(90_000); // one long walk through four settings pages
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  const nav = page.getByRole("navigation", { name: "Settings sections" });

  // Accounts: a pasted Jira link fills the site; the token is saved to the Keychain on save.
  await nav.getByRole("menuitem", { name: /Accounts/ }).click();
  await page.getByRole("button", { name: "Connect an account" }).click();
  await page.getByRole("menuitem", { name: /Atlassian/ }).click();
  let dialog = page.getByRole("dialog").last();
  await dialog.getByRole("textbox", { name: "Paste a link" }).fill("https://acme.atlassian.net/browse/OPS-12");
  await expect(dialog.getByPlaceholder("https://yourteam.atlassian.net", { exact: true })).toHaveValue("https://acme.atlassian.net");
  // A new account starts on the twg sign-in: no email or token to fill.
  await expect(dialog.getByPlaceholder("you@company.com")).toHaveCount(0);
  await dialog.getByText("API token", { exact: true }).click();
  await dialog.getByPlaceholder("you@company.com").fill("me@acme.test");
  await dialog.getByRole("textbox", { name: "API token" }).fill("plain-token-123");
  await dialog.getByRole("button", { name: "Check" }).click();
  await expect(dialog.getByText("Signed in as Demo Person")).toBeVisible();
  await dialog.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByText(/in the settings file as plain text/)).toBeVisible();
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByText(/Tokens moved to the Keychain: connection acme/)).toBeVisible();

  // Jira tracker: pick the account and the project, then statuses; the JQL is written for you.
  // Pasting a link is a folded shortcut, not a step.
  await nav.getByRole("menuitem", { name: /Trackers/ }).click();
  await page.getByRole("button", { name: "Add tracker" }).first().click();
  await page.getByRole("menuitem", { name: "Jira" }).click();
  dialog = page.getByRole("dialog").last();
  await expect(dialog.getByRole("textbox", { name: "Paste a link" })).toHaveCount(0);
  await expect(dialog.getByRole("button", { name: /Have a link\? Paste it/ })).toBeVisible();
  await dialog.getByRole("combobox", { name: "Account" }).click();
  await page.locator(".ant-select-dropdown:visible .ant-select-item-option").filter({ hasText: /^acme/ }).click();
  await dialog.getByRole("combobox", { name: "Project" }).click();
  await page.locator(".ant-select-dropdown:visible .ant-select-item-option").filter({ hasText: "Procure to pay" }).click();
  await expect(dialog.getByRole("textbox", { name: "Name" })).toHaveValue("jira-pmp");
  await dialog.getByRole("combobox", { name: "In status" }).click();
  await page.locator(".ant-select-item-option").filter({ hasText: "Selected for Development" }).click();
  await page.keyboard.press("Escape");
  await dialog.getByRole("combobox", { name: "When picked up, move to" }).click();
  await page.locator(".ant-select-dropdown:visible").last().locator(".ant-select-item-option").filter({ hasText: /^In Development$/ }).click();
  await dialog.getByRole("button", { name: /Advanced/ }).click();
  await expect(dialog.getByRole("textbox", { name: "JQL" })).toHaveValue('project = PMP AND status in ("Selected for Development") AND assignee = currentUser() ORDER BY priority DESC, updated DESC');
  await expect(dialog.getByRole("textbox", { name: "API token" })).toHaveValue("Stored in the Keychain");
  await dialog.getByRole("button", { name: "Add tracker" }).click();
  await expect(page.getByText("jira-pmp", { exact: true })).toBeVisible();

  // Knowledge over MCP: the server says which tools search and read.
  await nav.getByRole("menuitem", { name: /Knowledge/ }).click();
  await page.getByRole("button", { name: "Add source" }).first().click();
  await page.getByRole("menuitem", { name: "MCP server" }).click();
  dialog = page.getByRole("dialog").last();
  await dialog.getByPlaceholder("https://kb.example/mcp/").fill("https://kb.acme.test/mcp/");
  await dialog.getByRole("button", { name: "Find the tools" }).click();
  await expect(dialog.locator(".st-tools-line")).toContainText("kb_search");
  await expect(dialog.getByRole("textbox", { name: "Name" })).toHaveValue("kb");
  // A server that signs in with OAuth: the browser sign-in, same address, no token.
  await dialog.getByText("Browser (OAuth)").click();
  await expect(dialog.getByRole("textbox", { name: "Server URL" })).toHaveValue("https://kb.acme.test/mcp/");
  await expect(dialog.getByRole("textbox", { name: "Token" })).toHaveCount(0);
  await expect(dialog).toContainText("opens your browser to sign in");
  // Signing in keeps the dialog and what was typed in it.
  // (Find the tools already reached it, so the button offers to sign in again.)
  await expect(dialog.getByText(/Signed in · \d+ tools? on the server/)).toBeVisible();
  await dialog.getByRole("button", { name: /Sign in (now|again)/ }).click();
  await expect(dialog.getByText(/Signed in · \d+ tools? on the server/)).toBeVisible();
  await expect(dialog.getByRole("textbox", { name: "Name" })).toHaveValue("kb");
  await dialog.getByRole("button", { name: "Add source" }).click();
  await expect(page.getByText("kb.acme.test · browser sign-in")).toBeVisible();

  // Trigger: every weekday at 09:30, tell an agent; the stored lines are built from the picks.
  await nav.getByRole("menuitem", { name: /Triggers/ }).click();
  await page.getByRole("button", { name: "Add trigger" }).first().click();
  dialog = page.getByRole("dialog").last();
  await dialog.getByRole("textbox", { name: "Name" }).fill("standup");
  await dialog.getByLabel("At", { exact: true }).fill("09:30");
  await dialog.getByRole("combobox", { name: "Agent" }).click();
  await page.locator(".ant-select-item-option").filter({ hasText: /^builder/ }).click();
  await dialog.getByRole("textbox", { name: "Message" }).fill('Summarise yesterday, "short"');
  await expect(dialog.getByLabel("What this trigger does")).toHaveText("Every weekday at 09:30, tell builder: “Summarise yesterday, ”short””.");
  await dialog.getByRole("button", { name: /Advanced/ }).click();
  await expect(dialog.getByRole("textbox", { name: "When (raw)" })).toHaveValue("cron 30 9 * * 1-5");
  await expect(dialog.getByRole("textbox", { name: "Action (raw)" })).toHaveValue('send builder "Summarise yesterday, ”short”"');
  await dialog.getByText("A pull request opens").click();
  await dialog.getByRole("combobox", { name: "Repository" }).click();
  await page.locator(".ant-select-item-option").filter({ hasText: "hubstudio/app" }).click();
  await expect(dialog.getByRole("textbox", { name: "When (raw)" })).toHaveValue("pr hubstudio/app");
  await dialog.getByRole("button", { name: "Add trigger" }).click();
  await expect(page.getByText("New PR in hubstudio/app")).toBeVisible();
  expect(errors).toEqual([]);
});

test("review requests from GitHub queue for reviewers, as many at once as there are reviewer agents", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /Trackers/ }).click();
  await page.getByRole("button", { name: "Add tracker" }).first().click();
  await page.getByRole("menuitem", { name: "GitHub review requests" }).click();
  const dialog = page.getByRole("dialog").last();
  await expect(dialog.getByRole("textbox", { name: "Name" })).toHaveValue("reviews");
  await expect(dialog.getByText("1 agent with this role: one review at a time; the rest wait in Todo.")).toBeVisible();
  await expect(dialog.getByRole("spinbutton", { name: "Waiting or in review at once" })).toHaveValue("10");
  await expect(dialog.getByRole("combobox", { name: "When done, move to" })).toHaveCount(0);
  await dialog.getByRole("button", { name: "Add tracker" }).click();
  await expect(page.getByRole("button", { name: /^reviews GitHub review requests/ })).toContainText("every repository");
  expect(errors).toEqual([]);
});

test("feedback on my pull requests: rounds for a pr-fixer, from today on", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /Trackers/ }).click();
  await page.getByRole("button", { name: "Add tracker" }).first().click();
  await page.getByRole("menuitem", { name: "Feedback on my pull requests" }).click();
  const dialog = page.getByRole("dialog").last();
  await expect(dialog.getByRole("textbox", { name: "Name" })).toHaveValue("feedback");
  await expect(dialog.getByLabel("Feedback from")).toHaveValue(new Date().toLocaleDateString("sv"));
  await expect(dialog.getByText("No agent has the role “pr-fixer” yet: rounds wait in Todo until one does.")).toBeVisible();
  await expect(dialog.getByRole("spinbutton", { name: "Waiting or in review at once" })).toHaveCount(0);
  await dialog.getByRole("textbox", { name: "Ignore comments from" }).fill("auto-reviewer");
  await dialog.getByRole("button", { name: /Advanced/ }).click();
  await dialog.getByRole("button", { name: "Add tracker" }).click();
  await expect(page.getByRole("button", { name: /^feedback Feedback on my pull requests/ })).toContainText("every repository");
  expect(errors).toEqual([]);
});

test("the inbox lists the pull requests agents handle, the ones that need you first", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Inbox" }).click();
  const prs = page.getByRole("region", { name: "Pull requests" });
  const cards = prs.locator(".pr-card");
  await expect(cards).toHaveCount(4);
  await expect(cards.nth(0)).toContainText("Needs you");
  await expect(cards.nth(0)).toContainText("merge order with acme/web#103");
  await expect(cards.nth(1)).toContainText("Paused");
  await expect(cards.nth(2)).toContainText("Ready to approve");
  await expect(cards.nth(2)).not.toContainText("VERDICT");
  await expect(cards.nth(3)).toContainText("Waiting");
  await cards.nth(1).getByRole("button", { name: "One more round" }).click();
  await expect(cards.filter({ hasText: "acme/web#98" })).toContainText("Done");
  const ready = cards.filter({ hasText: "acme/api#51" });
  await ready.getByRole("button", { name: "Approve on GitHub" }).click();
  await expect(ready).toContainText("You approved it on GitHub.");
  await expect(ready.getByRole("button", { name: "Approve on GitHub" })).toHaveCount(0);
  await cards.filter({ hasText: "acme/web#107" }).getByRole("button", { name: "Ignore PR" }).click();
  await expect(cards.filter({ hasText: "acme/web#107" })).toHaveCount(0);
  await expect(prs.getByRole("button", { name: "Open acme/api#55 on GitHub" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("alerts can also go to a webhook, and review trackers review again after a push", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /Agents/ }).click();
  await expect(page.getByLabel("Alert webhook")).toBeVisible();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /Trackers/ }).click();
  await page.getByRole("button", { name: "Add tracker" }).first().click();
  await page.getByRole("menuitem", { name: "GitHub review requests" }).click();
  await expect(page.getByRole("dialog").last().getByRole("switch", { name: "Review again after a push" })).toBeChecked();
  expect(errors).toEqual([]);
});

test("skills: link one into every tool, sync all, add one, and copy one from another device", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /Skills/ }).click();
  const row = page.getByRole("listitem").filter({ hasText: "pr-review" });
  await expect(row.locator(".sk-chip.is-on")).toHaveText(["claude"]);
  await row.getByRole("button", { name: /Link to all tools/ }).click();
  await expect(row.locator(".sk-chip.is-on")).toHaveText(["~/.agents", "claude", "opencode", "pi"]);
  await page.getByRole("switch", { name: "Keep every tool in sync" }).click();
  await expect(page.getByRole("button", { name: "Sync now" })).toBeDisabled();
  await page.getByLabel("Skill to add").fill("https://github.com/acme/release-notes.git");
  await page.locator(".sk-add").getByRole("button").click();
  await expect(page.getByRole("listitem").filter({ hasText: "release-notes" }).locator(".sk-chip.is-on")).toHaveCount(4);
  await page.locator(".sk-tools").getByText("mini", { exact: true }).click();
  const qa = page.getByRole("listitem").filter({ hasText: "qa-automation" });
  await qa.getByRole("button", { name: "Copy to this device" }).click();
  await expect(qa.getByText("On this device")).toBeVisible();
  expect(errors).toEqual([]);
});

test("roles: edit the default text of a role, then go back to the default", async ({ page }) => {
  const errors = watchErrors(page);
  await open(page);
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("navigation", { name: "Settings sections" }).getByRole("menuitem", { name: /Roles/ }).click();
  const row = page.getByRole("listitem").filter({ has: page.getByRole("button", { name: "Edit the reviewer role" }) });
  await expect(row).toContainText("default");
  await row.getByRole("button", { name: "Edit the reviewer role" }).click();
  const box = page.getByRole("textbox", { name: "Text for the reviewer role" });
  await expect(box).toHaveValue(/Your job is to review what the engineers send you/);
  await box.fill((await box.inputValue()) + " Run the affected tests before you approve.");
  await page.getByRole("button", { name: "Save role" }).click();
  await expect(row).toContainText("edited");
  await row.getByRole("button", { name: "Edit the reviewer role" }).click();
  await expect(box).toHaveValue(/Run the affected tests before you approve\.$/);
  await page.getByRole("button", { name: "Back to the default" }).click();
  await expect(row).not.toContainText("edited");
  await row.getByRole("button", { name: "Edit the reviewer role" }).click();
  await expect(box).not.toHaveValue(/Run the affected tests/);
  expect(errors).toEqual([]);
});
