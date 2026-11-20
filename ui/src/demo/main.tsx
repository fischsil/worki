import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

import type { ProjectEnvironment } from "../lib/api";

const scene = new URLSearchParams(location.search).get("scene") ?? "main";

const EMPTY_CART = `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="480" height="270" font-family="-apple-system, sans-serif">
  <rect width="480" height="270" fill="#f6f6f7"/>
  <rect x="120" y="40" width="240" height="190" rx="12" fill="#fff" stroke="#e2e2e6"/>
  <text x="144" y="78" font-size="16" font-weight="600" fill="#1d1d20">Order summary</text>
  <text x="144" y="118" font-size="13" fill="#6b6b73">Subtotal</text><text x="336" y="118" font-size="13" fill="#1d1d20" text-anchor="end">$0.00</text>
  <text x="144" y="146" font-size="13" fill="#6b6b73">Shipping</text><text x="336" y="146" font-size="13" fill="#1d1d20" text-anchor="end">$0.00</text>
  <line x1="144" y1="164" x2="336" y2="164" stroke="#e2e2e6"/>
  <text x="144" y="194" font-size="14" font-weight="600" fill="#1d1d20">Total</text><text x="336" y="194" font-size="14" font-weight="600" fill="#d93025" text-anchor="end">$0.00</text>
</svg>`)}`;
const chatImages = [EMPTY_CART];
let dictation: ReturnType<typeof setTimeout>[] = [];
let stopDictation = () => {};
const ROOT = "/Users/dev/worki/worktrees";
const now = Math.floor(Date.now() / 1000);

const DEFAULT_CLAUDE_SETTINGS = {
  sandbox: {
    enabled: false,
    allowUnsandboxedCommands: true,
    network: { allowAllUnixSockets: true },
    filesystem: { allowWrite: ["/"] },
  },
};

let keepAwake = false;
let snapshotCleanup = "";

let agentSettings = {
  profiles: [
    { id: "claude", name: "Claude Code", command: "claude", install: null },
    { id: "codex", name: "Codex", command: "codex", install: null },
  ],
  defaultProfile: "claude",
  secrets: { global: ["GH_TOKEN"], "project:acme-web": ["STRIPE_TEST_KEY"] } as Record<string, string[]>,
  remoteControl: false,
  claudeSettings: DEFAULT_CLAUDE_SETTINGS,
  accounts: [
    { id: "work", email: "dev@acme.com", plan: "max", organization: "Acme" },
    { id: "side", email: "me@example.com", plan: "pro", organization: null },
  ] as { id: string; email: string | null; plan: string | null; organization: string | null }[],
  projectAccounts: { "acme-web": "side" } as Record<string, string>,
  defaultAccount: "work" as string | null,
  projectSignIn: {} as Record<string, "folder" | "token">,
};
const accountTokens = new Set<string>();
const projectEnvironments: Record<string, ProjectEnvironment> = {};
let finishLogin: (() => void) | null = null;

const projects = [
  project("acme-web", "/Users/dev/src/acme-web"),
  project("payments-api", "/Users/dev/src/payments-api"),
  project("infra", "/Users/dev/src/infra"),
];

function project(name: string, repoPath: string) {
  return {
    id: name,
    name,
    repoPath,
    worktreeRoot: `${ROOT}/${name}`,
    defaultBase: "main",
    isolation: { kind: "inherit" },
    setupScript: null,
    remote: null,
  };
}

const wt = (p: string, branch: string | null, isMain = false) => ({
  path: isMain ? projects.find((x) => x.id === p)!.repoPath : `${ROOT}/${p}/${branch!.replace(/\//g, "-")}`,
  branch: branch ?? "main",
  base: null as string | null,
  head: "abc1234",
  isMain,
  locked: false,
  prunable: false,
});

let prBase = "main";

const worktrees: Record<string, ReturnType<typeof wt>[]> = {
  "acme-web": [wt("acme-web", null, true), wt("acme-web", "feat/checkout-redesign"), wt("acme-web", "fix/login-race"), wt("acme-web", "chore/react-19")],
  "payments-api": [wt("payments-api", null, true), wt("payments-api", "feat/refund-webhooks")],
  infra: [wt("infra", null, true), wt("infra", "feat/staging-cluster")],
};

const checkout = worktrees["acme-web"][1].path;
const login = worktrees["acme-web"][2].path;
const refunds = worktrees["payments-api"][1].path;
const staging = worktrees.infra[1].path;

const file = (path: string, code: string, added = 0, removed = 0) => ({
  path,
  code,
  lines: { added, removed },
});
const status: Record<string, unknown> = {
  [checkout]: {
    changedFiles: [
      file("src/checkout/Summary.tsx", " M", 12, 5),
      file("src/checkout/summary.css", "??", 38),
      file("src/checkout/Summary.test.tsx", " M", 24, 2),
    ],
    insertions: 61,
    deletions: 9,
    ahead: 2,
    behind: 0,
    upstream: "origin/feat/checkout-redesign",
    conflicts: [],
    stopped: null,
  },
  [login]: { changedFiles: [file("src/auth/session.ts", " M")], insertions: 12, deletions: 4, ahead: 0, behind: 0, upstream: null, conflicts: [], stopped: null },
};
const clean = { changedFiles: [], insertions: 0, deletions: 0, ahead: 0, behind: 0, upstream: "origin/main", conflicts: [], stopped: null };

const commits = [
  { sha: "a1b2c3d4", subject: "Show a skeleton while the cart loads", author: "Claude", time: now - 25 * 60 },
  { sha: "b2c3d4e5", subject: "Render the checkout summary as a card", author: "Claude", time: now - 70 * 60 },
  { sha: "c3d4e5f6", subject: "Split shipping and discount into their own rows", author: "Claude", time: now - 3 * 3600 },
];

const shellPrompt = "\x1b[32mdev@mac\x1b[0m \x1b[34m~/worki/worktrees/acme-web/checkout-redesign\x1b[0m $ pnpm test\r\n\r\n \x1b[32m✓\x1b[0m 128 tests passed (2.4s)\r\n\r\n\x1b[32mdev@mac\x1b[0m \x1b[34m~/worki/worktrees/acme-web/checkout-redesign\x1b[0m $ ";


const isolation = { kind: "dockerSandbox", template: "worki-acme-web:20261109-1430" };

const session = (id: string, path: string, activity: string, extra: object = {}) => ({
  id,
  kind: "terminal",
  worktreePath: path,
  profileId: "claude",
  title: "Claude Code",
  runAs: "sbx: worki-" + path.split("/").pop(),
  resumable: false,
  panel: false,
  state: { kind: "running" },
  activity,
  attention: null,
  createdAt: now,
  usage: [{ model: "claude-opus-5-5", input: 2400, output: 38_000, cacheWrite: 120_000, cacheWrite1h: 0, cacheRead: 1_400_000 }],
  agent: {
    model: "Opus 5.5",
    effort: "high",
    fastMode: false,
    thinking: true,
    permissionMode: "acceptEdits",
    contextUsed: 34,
    contextSize: 200_000,
    costUsd: 1.84,
    outputStyle: "default",
    version: "2.1.319",
  },
  ...extra,
});

const sessions = [
  session("s1", checkout, "working", { account: { email: "me@example.com", plan: "pro", organization: null, token: false, tokenName: null } }),
  session("s2", login, "waiting", { attention: "Claude needs your permission to use Bash" }),
  session("s3", refunds, "working"),
  session("s4", staging, "waiting", { attention: "Which region should the staging cluster use?" }),
  session("s5", checkout, scene === "hero" ? "working" : "waiting", {
    kind: "chat",
    attention: scene === "hero" ? null : "Allow Bash?",
    createdAt: now + 1,
    agent: {
      model: "Opus 5.5",
      effort: "high",
      permissionMode: "default",
      contextUsed: 34,
      contextSize: 200_000,
      costUsd: 1.84,
      version: "2.1.319",
      account: { email: "dev@acme.com", plan: "Claude Max", organization: "Acme", token: false, tokenName: null },
      remoteControl:
        scene === "hero" || scene === "chat"
          ? { state: "connected", url: "https://claude.ai/code/session_demo" }
          : { state: "off", url: null },
    },
  }),
];

const bash = (id: string, description: string, command: string, output: string | null, background = false) => [
  { type: "toolCall", id, name: "Bash", parent: null, input: { command, description, ...(background && { run_in_background: true }) } },
  ...(output === null ? [] : [{ type: "toolResult", id, parent: null, isError: false, output }]),
];

const timingChart = `<div class="c"><div class="h"><b>Where the checkout tests spend their time</b><span>2.41 s in total</span></div>${[
  ["Summary", 52, "1.26 s"],
  ["Cart", 24, "0.58 s"],
  ["LineItem", 14, "0.33 s"],
  ["Shipping", 10, "0.24 s"],
]
  .map(([name, share, time]) => `<div class="r"><span>${name}</span><div class="t"><i style="--w:${share}%"></i></div><b>${time}</b></div>`)
  .join("")}</div><style>.c{padding:18px 20px;border:1px solid var(--line);border-radius:14px;background:var(--elevated);font:13px/1.4 -apple-system,system-ui,sans-serif}.h{display:flex;justify-content:space-between;align-items:baseline;gap:12px;margin-bottom:14px;color:var(--muted)}.h b{font-size:14px;font-weight:600;color:var(--fg)}.r{display:grid;grid-template-columns:72px 1fr 52px;gap:14px;align-items:center;margin:10px 0;color:var(--muted)}.t{height:8px;border-radius:4px;background:var(--hover)}.t i{display:block;height:100%;width:var(--w);border-radius:4px;background:linear-gradient(90deg,color-mix(in srgb,var(--accent) 55%,transparent),var(--accent))}.r b{font-weight:500;color:var(--fg);text-align:right;font-variant-numeric:tabular-nums}</style>`;

const devServer = { id: "b1", kind: "local_bash", description: "Start the dev server" };
const testWatcher = { id: "b2", kind: "local_bash", description: "Watch the checkout tests" };
const fixtureAgent = { id: "b3", kind: "local_agent", description: "Look for slow fixtures in the other suites" };

/** The website's first screenshot: a chat at work, with jobs running in the background. Each
 * step is stamped as if it happened `ago` seconds earlier, so the timers show real-looking times. */
const heroSteps: [number, object[]][] = [
  [
    150,
    [
      { type: "session", model: "claude-opus-5-5", permissionMode: "acceptEdits", effort: "high" },
      {
        type: "user",
        text: "The checkout tests got slow. Find out where the time goes and fix it. Keep the dev server and a test watcher running while you work.",
        attachments: [],
      },
      { type: "text", id: "h1:0", parent: null, text: "I'll start the dev server and a test watcher in the background, then profile the suite." },
    ],
  ],
  [140, [...bash("h2", "Start the dev server", "pnpm dev", "Command running in background with ID: b1", true), { type: "backgroundTasks", tasks: [devServer] }]],
  [
    132,
    [
      ...bash("h3", "Watch the checkout tests", "pnpm vitest src/checkout", "Command running in background with ID: b2", true),
      { type: "backgroundTasks", tasks: [devServer, testWatcher] },
      ...bash("h4", "Profile the checkout tests", "pnpm vitest run src/checkout --reporter=json", "4 test files, 31 tests, 2.41 s"),
      {
        type: "text",
        id: "h5:0",
        parent: null,
        text: `Half the time goes to \`Summary\`:\n\n\`\`\`html\n${timingChart}\n\`\`\`\n\nIt renders the whole cart again for every case. I'll share one fixture and mock the price API, and have an agent check the other suites for the same pattern.`,
      },
    ],
  ],
  [
    70,
    [
      { type: "backgroundTasks", tasks: [devServer, testWatcher, fixtureAgent] },
      {
        type: "toolCall",
        id: "h6",
        name: "Edit",
        parent: null,
        input: { file_path: `${checkout}/src/checkout/Summary.test.tsx`, old_string: "render(<Summary cart={makeCart()} />)", new_string: "render(<Summary cart={cart} />)" },
      },
      { type: "toolResult", id: "h6", parent: null, isError: false, output: "The file was updated." },
    ],
  ],
  [24, bash("h7", "Run the checkout tests with timings", "pnpm vitest run src/checkout --reporter=verbose", null)],
];

const chatEvents: object[] = [
  {
    type: "options",
    permissionModes: ["default", "acceptEdits", "plan", "auto"],
    models: [
      { value: "default", model: "claude-opus-5-5", label: "Default (recommended)", description: "Opus 5.5", effortLevels: ["low", "medium", "high", "xhigh", "max"] },
      { value: "opus", model: "claude-opus-5-5", label: "Opus 5.5", description: "For complex work", effortLevels: ["low", "medium", "high", "xhigh", "max"] },
      { value: "sonnet", model: "claude-sonnet-5-5", label: "Sonnet 5.5", description: "Fast everyday coding", effortLevels: ["low", "medium", "high", "xhigh", "max"] },
      { value: "haiku", model: "claude-haiku-5-5", label: "Haiku 5.5", description: "Fastest", effortLevels: ["low", "medium", "high"] },
    ],
  },
  {
    type: "commands",
    commands: [
      { trigger: "/compact", description: "Clear the conversation history but keep a summary in context", argumentHint: "<optional custom summarization instructions>" },
      { trigger: "/context", description: "Show current context usage", argumentHint: null },
      { trigger: "/cost", description: "Show the total cost and duration of the current session", argumentHint: null },
      { trigger: "/init", description: "Initialize a new CLAUDE.md file with codebase documentation", argumentHint: null },
      { trigger: "/review", description: "Review a pull request", argumentHint: "<pr number>" },
      { trigger: "/security-review", description: "Complete a security review of the pending changes on the current branch", argumentHint: null },
      { trigger: "/simplify", description: "Review the changed code for reuse, simplification and efficiency, then apply the fixes", argumentHint: null },
    ],
  },
  { type: "session", model: "claude-opus-5-5", permissionMode: "default", effort: "high" },
  {
    type: "user",
    text: "The empty cart shows a total of $0.00 with shipping. Hide the summary when the cart is empty and add a test.",
    attachments: [{ name: "empty-cart.png", image: 0 }],
  },
  { type: "text", id: "m1:0", parent: null, text: "I'll look at how the summary handles an empty cart first." },
  { type: "toolCall", id: "t1", name: "Read", parent: null, input: { file_path: `${checkout}/src/checkout/Summary.tsx` } },
  { type: "toolResult", id: "t1", parent: null, isError: false, output: "export function Summary({ cart }: Props) {\n  const subtotal = …" },
  { type: "backgroundTasks", tasks: [{ id: "b1", kind: "local_bash", description: "Start the dev server" }] },
  {
    type: "toolCall",
    id: "t2",
    name: "Edit",
    parent: null,
    input: {
      file_path: `${checkout}/src/checkout/Summary.tsx`,
      old_string: "export function Summary({ cart }: Props) {",
      new_string: "export function Summary({ cart }: Props) {\n  if (cart.items.length === 0) return null;",
    },
  },
  { type: "toolResult", id: "t2", parent: null, isError: false, output: "The file was updated." },
  {
    type: "text",
    id: "m2:0",
    parent: null,
    text: "The summary now renders nothing for an **empty cart**. I added a test:\n\n- `hides the summary for an empty cart`\n\nNext I'll run the checkout tests.",
  },
  {
    type: "text",
    id: "m2b:0",
    parent: null,
    text: "How the summary decides what to show:\n\n```mermaid\nflowchart LR\n  A[Cart] --> B{Items?}\n  B -- none --> C[Hide summary]\n  B -- some --> D[Subtotal + shipping]\n  D --> E[Total]\n```\n\nAnd the card layout:\n\n```svg\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"240\" height=\"90\" viewBox=\"0 0 240 90\"><rect x=\"4\" y=\"4\" width=\"232\" height=\"82\" rx=\"8\" fill=\"#f6f8fa\" stroke=\"#d0d7de\"/><text x=\"16\" y=\"30\" font-size=\"13\" fill=\"#57606a\">Subtotal</text><text x=\"16\" y=\"52\" font-size=\"13\" fill=\"#57606a\">Shipping</text><text x=\"16\" y=\"74\" font-size=\"14\" font-weight=\"bold\" fill=\"#24292f\">Total</text><script>alert(1)</script></svg>\n```",
  },
  {
    type: "text",
    id: "m2c:0",
    parent: null,
    text: "Test timing per file:\n\n```html\n<div class=\"chart\"><h4>Checkout test time (ms)</h4><div class=\"row\"><span>Summary</span><i style=\"--w:82%\"></i><b>431</b></div><div class=\"row\"><span>LineItem</span><i style=\"--w:36%\"></i><b>188</b></div><div class=\"row\"><span>Cart</span><i style=\"--w:58%\"></i><b>302</b></div></div><style>.chart{padding:14px 16px;border:1px solid var(--line);border-radius:10px;background:var(--elevated)}h4{margin:0 0 10px;font-weight:600;color:var(--fg)}.row{display:grid;grid-template-columns:72px 1fr 40px;align-items:center;gap:10px;margin:6px 0;color:var(--muted)}i{display:block;height:10px;width:var(--w);border-radius:5px;background:var(--accent)}b{font-weight:500;color:var(--fg);text-align:right}</style>\n```",
  },
  {
    type: "permission",
    requestId: "p1",
    tool: "Bash",
    input: { command: "pnpm vitest run src/checkout", description: "Run the checkout tests" },
    questions: [],
  },
];
type DemoChannel = { onmessage?: (data: number[]) => void };
let chatSettings: Record<string, string> = { model: "claude-opus-5-5", permissionMode: "default", effort: "high" };
let chatChannel: DemoChannel | null = null;
let sentMessages = 0;
let shellCount = 0;
const sendTo = (channel: DemoChannel | null, events: object[]) =>
  channel?.onmessage?.(Array.from(new TextEncoder().encode(events.map((e) => JSON.stringify(e) + "\n").join(""))));
const sendChat = (...events: object[]) => sendTo(chatChannel, events);

const esc = "\x1b[";
const c = (code: string, text: string) => `${esc}${code}m${text}${esc}0m`;
const terminal = [
  `${c("38;5;209", "✻")} ${c("1", "Welcome to Claude Code")}`,
  `  ${c("90", checkout)}`,
  "",
  `${c("1", ">")} Redesign the checkout summary as a card with subtotal, shipping and`,
  `  total rows, show a skeleton while loading, and keep the tests green.`,
  "",
  `${c("38;5;209", "●")} I'll start with the current summary and its tests.`,
  "",
  `${c("32", "●")} ${c("1", "Read")}(src/checkout/Summary.tsx)`,
  `  ${c("90", "└ Read 24 lines")}`,
  `${c("32", "●")} ${c("1", "Update")}(src/checkout/Summary.tsx)`,
  `  ${c("90", "└ Updated src/checkout/Summary.tsx with 21 additions and 7 removals")}`,
  `     ${c("90", " 8")} ${c("31", "-  const total = cart.items.reduce((sum, i) => sum + i.price * i.qty, 0);")}`,
  `     ${c("90", " 8")} ${c("32", "+  const subtotal = cart.items.reduce((sum, i) => sum + i.price * i.qty, 0);")}`,
  `     ${c("90", " 9")} ${c("32", "+  const total = subtotal + cart.shipping - cart.discount;")}`,
  `     ${c("90", "11")} ${c("32", "+  if (loading) return <LineItemSkeleton rows={3} />;")}`,
  "",
  `${c("32", "●")} ${c("1", "Bash")}(pnpm vitest run src/checkout)`,
  `  ${c("90", "└")} ${c("32", "✓")} src/checkout/Summary.test.tsx ${c("90", "(9 tests) 412ms")}`,
  `    ${c("32", "✓")} src/checkout/LineItem.test.tsx ${c("90", "(5 tests) 188ms")}`,
  `    ${c("1", "Test Files")}  ${c("32", "2 passed")} ${c("90", "(2)")}`,
  "",
  `${c("38;5;209", "✻ Polishing the empty-cart state…")} ${c("90", "(38s · ↓ 2.1k tokens · esc to interrupt)")}`,
  "",
].join("\r\n");

const diff = `diff --git a/src/checkout/Summary.tsx b/src/checkout/Summary.tsx
--- a/src/checkout/Summary.tsx
+++ b/src/checkout/Summary.tsx
@@ -1,20 +1,26 @@
 import { useCart } from "../cart";
 import { Money } from "../ui/Money";
+import { LineItemSkeleton } from "./LineItem";
+import "./summary.css";
 
 export function Summary() {
-  const { cart } = useCart();
-  const total = cart.items.reduce((sum, i) => sum + i.price * i.qty, 0);
+  const { cart, loading } = useCart();
+  const subtotal = cart.items.reduce((sum, i) => sum + i.price * i.qty, 0);
+  const total = subtotal + cart.shipping - cart.discount;
+
+  if (loading) return <LineItemSkeleton rows={3} />;
 
   return (
-    <div className="summary">
-      <Money value={total} />
-    </div>
+    <section className="summary-card">
+      <Row label="Subtotal" value={subtotal} />
+      <Row label="Shipping" value={cart.shipping} />
+      <Row label="Total" value={total} strong />
+    </section>
   );
 }
`;

const apps = [
  { id: "vscode", name: "Visual Studio Code", path: "/Applications/Visual Studio Code.app" },
  { id: "idea", name: "IntelliJ IDEA", path: "/Applications/IntelliJ IDEA.app" },
];

const check = (id: string, label: string) => ({ id, label, ok: true, detail: null, fix: null });

const info = (line: string) => ({ stream: "info", line });
const at = (agoMs: number) => Date.now() - agoMs;
const activities = [
  {
    id: 3,
    title: "Start Claude Code",
    projectId: "infra",
    worktreePath: staging,
    status: "running",
    startedAt: at(9_000),
    finishedAt: null,
    error: null,
    lines: [
      info("Checking Docker (sbx) is ready…"),
      { stream: "command", line: "sbx version" },
      info("Getting the worktree's container ready…"),
      { stream: "command", line: "sbx ls --json" },
      { stream: "command", line: "sbx run -d worki-infra-feat-staging-cluster" },
      info("Checking Claude Code is installed…"),
      { stream: "command", line: "sbx exec worki-infra-feat-staging-cluster bash -lc 'command -v claude'" },
      info("Claude Code isn't installed here yet. Installing it…"),
      { stream: "command", line: "sbx exec worki-infra-feat-staging-cluster bash -lc 'curl -fsSL https://claude.ai/install.sh | bash'" },
    ],
  },
  {
    id: 2,
    title: "Create worktree fix/login-race",
    projectId: "acme-web",
    worktreePath: login,
    status: "done",
    startedAt: at(120_000),
    finishedAt: at(112_000),
    error: null,
    lines: [
      { stream: "command", line: "git -C /Users/dev/src/acme-web show-ref --verify --quiet refs/heads/fix/login-race" },
      { stream: "command", line: "git -C /Users/dev/src/acme-web rev-parse --verify --quiet 'main^{commit}'" },
      info("Creating worktree fix/login-race from main…"),
      { stream: "command", line: "git -C /Users/dev/src/acme-web worktree add --no-track -b fix/login-race /Users/dev/worki/worktrees/acme-web/fix-login-race main" },
      info("Running setup script as you…"),
      { stream: "stdout", line: "added 812 packages in 6s" },
      info("Done."),
    ],
  },
  {
    id: 1,
    title: "Save snapshot node-22",
    projectId: "payments-api",
    worktreePath: refunds,
    status: "failed",
    startedAt: at(600_000),
    finishedAt: at(559_000),
    error: "docker commit: no space left on device",
    lines: [info("Saving the worktree's environment as an image…")],
  },
];

function handle(cmd: string, args: Record<string, any>): unknown {
  switch (cmd) {
    case "list_projects":
      return projects;
    case "reorder_projects":
      projects.sort((a, b) => args.ids.indexOf(a.id) - args.ids.indexOf(b.id));
      return null;
    case "update_project": {
      const project = projects.find((p) => p.id === args.id);
      if (project) Object.assign(project, args.patch);
      return project;
    }
    case "list_worktrees":
      return worktrees[args.projectId] ?? [];
    case "worktree_status":
      return status[args.path] ?? clean;
    case "list_sessions":
      return sessions;
    case "agent_settings":
      return agentSettings;
    case "save_agent_profiles":
      agentSettings = { ...agentSettings, profiles: args.profiles, defaultProfile: args.defaultProfile };
      return null;
    case "set_remote_control":
      agentSettings = { ...agentSettings, remoteControl: args.enabled };
      return null;
    case "set_claude_settings":
      agentSettings = { ...agentSettings, claudeSettings: args.settings };
      return null;
    case "add_claude_account": {
      const account = { id: crypto.randomUUID(), email: null, plan: null, organization: null };
      agentSettings = { ...agentSettings, accounts: [...agentSettings.accounts, account] };
      return account;
    }
    case "set_default_account":
      agentSettings = { ...agentSettings, defaultAccount: args.id };
      return null;
    case "remove_claude_account":
      agentSettings = { ...agentSettings, accounts: agentSettings.accounts.filter((a) => a.id !== args.id) };
      return null;
    case "obtain_account_token":
      setTimeout(
        () => args.onEvent.onmessage?.({ kind: "url", url: "https://claude.ai/oauth/authorize?code=true" }),
        800,
      );
      return new Promise<null>((resolve) => {
        finishLogin = () => {
          accountTokens.add(args.id);
          resolve(null);
        };
      });
    case "log_in_claude_account":
      setTimeout(
        () => args.onEvent.onmessage?.({ kind: "url", url: "https://claude.ai/oauth/authorize?code=true" }),
        800,
      );
      return new Promise((resolve) => {
        finishLogin = () => {
          const signedIn = { email: "new@example.com", plan: "max", organization: null };
          agentSettings = {
            ...agentSettings,
            accounts: agentSettings.accounts.map((a) => (a.id === args.id ? { ...a, ...signedIn } : a)),
          };
          resolve(agentSettings.accounts.find((a) => a.id === args.id));
        };
      });
    case "enter_login_code":
      setTimeout(() => finishLogin?.(), 1000);
      return null;
    case "cancel_login":
      return null;
    case "set_project_account": {
      const projectAccounts = { ...agentSettings.projectAccounts };
      if (args.accountId) projectAccounts[args.projectId] = args.accountId;
      else delete projectAccounts[args.projectId];
      agentSettings = { ...agentSettings, projectAccounts };
      return null;
    }
    case "set_project_sign_in": {
      const projectSignIn: Record<string, "folder" | "token"> = { ...agentSettings.projectSignIn, [args.projectId]: args.signIn };
      if (args.signIn === "folder") delete projectSignIn[args.projectId];
      agentSettings = { ...agentSettings, projectSignIn };
      return null;
    }
    case "project_claude_dir":
      return `/Users/dev/Library/Application Support/worki/projects/${args.projectId}/claude`;
    case "project_environment":
      return projectEnvironments[args.projectId] ?? { sharedFolder: false, claudeFiles: "project" };
    case "set_project_environment":
      projectEnvironments[args.projectId] = args.environment;
      return null;
    case "project_shared_dir":
      return `/Users/dev/Library/Application Support/worki/projects/${args.projectId}/shared`;
    case "account_token_set":
      return accountTokens.has(args.accountId);
    case "set_account_token":
      if (args.token) accountTokens.add(args.accountId);
      else accountTokens.delete(args.accountId);
      return null;
    case "activities":
      return activities;
    case "keep_awake":
      return keepAwake;
    case "snapshot_cleanup":
      return snapshotCleanup;
    case "set_snapshot_cleanup":
      snapshotCleanup = args.script;
      return null;
    case "set_keep_awake":
      keepAwake = args.on;
      return null;
    case "default_claude_settings":
      return DEFAULT_CLAUDE_SETTINGS;
    case "host_claude_settings":
      return {
        model: "sonnet",
        theme: "auto",
        enabledPlugins: { "egm@egm": true },
        permissions: { defaultMode: "auto" },
      };
    case "isolation_settings":
      return { default: isolation, stopOnQuit: false };
    case "worktree_isolation":
      return { choice: { kind: "inherit" }, effective: isolation };
    case "opener_settings":
      return { apps, defaultApp: "vscode" };
    case "app_icon":
      return null;
    case "compare_base":
      return args.base;
    case "retarget_branch":
      for (const [id, list] of Object.entries(worktrees))
        worktrees[id] = list.map((w) => (w.path === args.worktreePath ? { ...w, base: args.base } : w));
      return null;
    case "branch_changes":
      return {
        base: args.base,
        files: [
          file("src/checkout/Summary.tsx", "M ", 12, 5),
          file("src/checkout/LineItem.tsx", "M ", 41, 17),
          file("src/checkout/summary.css", "A ", 38),
          file("src/checkout/Summary.test.tsx", "M ", 51, 15),
        ],
        insertions: 142,
        deletions: 37,
        commits: 3,
        behind: 2,
      };
    case "branch_log":
      return args.worktreePath === checkout ? commits : args.worktreePath === login ? commits.slice(-1) : [];
    case "snapshots":
      return [
        { engine: "dockerSandbox", name: "worki-acme-web:20261104-1012", created: 1793783520, size: 4_020_000_000, usedBy: ["acme-web"] },
        { engine: "dockerSandbox", name: "worki-acme-web:20261020-0931", created: 1792488660, size: 3_610_000_000, usedBy: [] },
        { engine: "docker", name: "worki-payments-api:20261101-1630", created: 1793547000, size: 1_240_000_000, usedBy: ["payments-api › feat-refund-webhooks"] },
        { engine: "docker", name: "worki-infra:20260930-0815", created: 1790748900, size: 612_000_000, usedBy: [] },
      ];
    case "delete_snapshot":
      return null;
    case "draft_commits": {
      const files = (args as { files: string[] }).files;
      const split = files.length > 1;
      return new Promise((resolve) =>
        setTimeout(
          () =>
            resolve(
              split
                ? [
                    { message: "Serialize session refreshes behind the login", files: files.slice(0, -1) },
                    { message: "Fix a typo in the checkout summary", files: files.slice(-1) },
                  ]
                : [{ message: "Serialize session refreshes behind the login", files }],
            ),
          900,
        ),
      );
    }
    case "draft_pull_request":
      return new Promise((resolve) =>
        setTimeout(
          () =>
            resolve({
              title: "Fix the race between login and session refresh",
              body: "## Summary\nLogging in while a session refresh was in flight could drop the new token.\n\n- Serialize refreshes behind the login\n- Add a regression test",
            }),
          1200,
        ),
      );
    case "pr_review_comments":
      return [];
    case "pr_failed_checks":
      return [
        {
          name: "build",
          url: "https://github.com/acme/web/actions/runs/1/job/2",
          description: "",
          log: "> Task :web:test\n\nCartSummary > renders the total in the user's currency FAILED\n    AssertionError: expected '$12.00' to equal 'CHF 12.00'\n\n1 failed, 214 passed\nProcess completed with exit code 1.",
        },
      ];
    case "open_pull_requests":
      return args.projectId === "acme-web"
        ? [
            { number: 47, title: "Add dark mode to the settings page", url: "https://github.com/acme/web/pull/47", author: "ana", branch: "settings-dark-mode", base: "main", approved: true, authored: false },
            { number: 46, title: "Cache product images", url: "https://github.com/acme/web/pull/46", author: "you", branch: "image-cache", base: "main", approved: false, authored: true },
            { number: 44, title: "Upgrade to React 19", url: "https://github.com/acme/web/pull/44", author: "ana", branch: "chore/react-19", base: "main", approved: true, authored: false },
            { number: 42, title: "Redesign the checkout summary", url: "https://github.com/acme/web/pull/42", author: "you", branch: "feat/checkout-redesign", base: "main", approved: true, authored: true },
          ]
        : [];
    case "set_pull_request_base":
      prBase = args.base;
      return null;
    case "pull_request":
      if (args.worktreePath === checkout)
        return { number: 42, title: "Redesign the checkout summary", url: "https://github.com/acme/web/pull/42", state: "OPEN", base: prBase, checksPassed: 3, checksFailed: 0, checksPending: 1 };
      if (args.worktreePath === worktrees["acme-web"][2].path)
        return { number: 39, title: "Fix the login race", url: "https://github.com/acme/web/pull/39", state: "MERGED", base: "main", checksPassed: 3, checksFailed: 0, checksPending: 0 };
      if (args.worktreePath === worktrees["acme-web"][3].path)
        return { number: 44, title: "Upgrade to React 19", url: "https://github.com/acme/web/pull/44", state: "OPEN", base: "main", checksPassed: 3, checksFailed: 0, checksPending: 0 };
      return null;
    case "file_diff":
      return diff;
    case "environment_templates":
      return ["worki-acme-web:20261109-1430"];
    case "isolation_doctor":
      return [
        check("binary", "Docker (sbx) is installed"),
        check("daemon", "The sandbox daemon is running"),
        check("image", "Template worki-acme-web:20261109-1430 is available"),
      ];
    case "agent_statuses":
      return [
        { profileId: "claude", name: "Claude Code", executable: "claude", envPath: "/usr/local/bin/claude", hostPath: "/opt/homebrew/bin/claude", canShare: false, auth: { secret: "CLAUDE_CODE_OAUTH_TOKEN", set: true } },
        { profileId: "codex", name: "Codex", executable: "codex", envPath: null, hostPath: "/opt/homebrew/bin/codex", canShare: false, auth: null },
      ];
    case "secret_status":
      return (args.names as string[]).map((n) => [n, true]);
    case "set_secret": {
      const names = agentSettings.secrets[args.scope] ?? [];
      if (!names.includes(args.name))
        agentSettings = { ...agentSettings, secrets: { ...agentSettings.secrets, [args.scope]: [...names, args.name] } };
      return null;
    }
    case "delete_secret":
      agentSettings = {
        ...agentSettings,
        secrets: {
          ...agentSettings.secrets,
          [args.scope]: (agentSettings.secrets[args.scope] ?? []).filter((n) => n !== args.name),
        },
      };
      return null;
    case "ssh_hosts":
      return [];
    case "setup_scripts":
      return { project: null, repoFile: "pnpm install" };
    case "spawn_worktree_shell": {
      const shell = {
        ...session(`panel-shell-${++shellCount}`, args.worktreePath, "idle"),
        profileId: null,
        title: "Terminal",
        runAs: null,
        panel: true,
        usage: [],
        createdAt: Date.now(),
      };
      sessions.push(shell as unknown as (typeof sessions)[number]);
      return shell;
    }
    case "close_session": {
      const i = sessions.findIndex((s) => s.id === args.id);
      if (i >= 0) sessions.splice(i, 1);
      return null;
    }
    case "attach_session": {
      const channel = args.onData;
      if (args.id.startsWith("panel-shell"))
        setTimeout(() => channel.onmessage?.(Array.from(new TextEncoder().encode(shellPrompt))), 50);
      if (args.id === "s1") setTimeout(() => channel.onmessage?.(Array.from(new TextEncoder().encode(terminal))), 50);
      if (args.id === "s5") {
        chatChannel = channel;
        const hero = [
          ...chatEvents.slice(0, 1),
          ...heroSteps.flatMap(([ago, events]) => events.map((e) => ({ ...e, at: Date.now() - ago * 1000 }))),
        ];
        setTimeout(() => sendTo(channel, scene === "hero" ? hero : chatEvents), 50);
      }
      return 1;
    }
    case "dictation_access":
      return true;
    case "start_dictation": {
      const words = "Also hide the shipping row when the cart is empty".split(" ");
      dictation = words.map((_, i) =>
        setTimeout(() => args.onEvent.onmessage?.({ type: "text", text: words.slice(0, i + 1).join(" "), done: false }), 300 * (i + 1)),
      );
      stopDictation = () => args.onEvent.onmessage?.({ type: "text", text: `${words.join(" ")}.`, done: true });
      return null;
    }
    case "stop_dictation":
      dictation.forEach(clearTimeout);
      setTimeout(stopDictation, 200);
      return null;
    case "cancel_dictation":
      dictation.forEach(clearTimeout);
      return null;
    case "chat_image":
      return chatImages[args.image];
    case "send_chat_message": {
      const id = `m${++sentMessages}`;
      sendChat(
        { type: "user", id, text: args.text, attachments: (args.attachments ?? []).map((a: { path?: string; name?: string; mediaType?: string; data?: string }) => ({
          name: a.name ?? a.path?.split("/").pop(),
          image: a.data ? chatImages.push(`data:${a.mediaType};base64,${a.data}`) - 1 : null,
        })) },
        { type: "userRead", id },
      );
      const s = sessions.find((s) => s.id === args.id);
      const setActivity = (activity: string) =>
        s &&
        import("@tauri-apps/api/event").then(({ emit }) => {
          Object.assign(s, { activity, attention: null });
          emit("session-changed", s);
        });
      setActivity("working");
      let i = 0;
      const words = "Sure — I'll take care of that next.".split(" ");
      const tick = () => {
        if (i < words.length) {
          sendChat({ type: "textDelta", id: `r${now}`, parent: null, text: (i ? " " : "") + words[i++] });
          setTimeout(tick, 120);
        } else {
          sendChat({ type: "turnEnded", ok: true, interrupted: false, error: null });
          setActivity("idle");
        }
      };
      setTimeout(tick, 2500);
      return null;
    }
    case "answer_chat_permission": {
      const allowed = args.answer.kind !== "deny";
      sendChat({ type: "permissionResolved", requestId: args.requestId, allowed });
      if (allowed)
        sendChat(
          { type: "toolCall", id: "t3", name: "Bash", parent: null, input: { command: "pnpm vitest run src/checkout", description: "Run the checkout tests" } },
          { type: "toolResult", id: "t3", parent: null, isError: false, output: " ✓ src/checkout/Summary.test.tsx (10 tests) 431ms\n Test Files  1 passed (1)" },
          { type: "text", id: "m3:0", parent: null, text: "All 10 checkout tests pass." },
          { type: "turnEnded", ok: true, interrupted: false, error: null },
          { type: "suggestion", text: "Commit the empty cart fix" },
        );
      const s = sessions.find((s) => s.id === args.id);
      if (allowed && s)
        import("@tauri-apps/api/event").then(({ emit }) => {
          Object.assign(s, { activity: "idle", attention: null });
          emit("session-changed", s);
        });
      return null;
    }
    case "change_chat_setting": {
      if (args.setting.kind === "remoteControl") {
        const s = sessions.find((s) => s.id === args.id)!;
        const report = (remoteControl: object) =>
          import("@tauri-apps/api/event").then(({ emit }) => {
            s.agent = { ...s.agent, remoteControl } as typeof s.agent;
            emit("session-changed", s);
          });
        if (!args.setting.value) report({ state: "off", url: null });
        else {
          report({ state: "connecting", url: null });
          setTimeout(() => report({ state: "connected", url: "https://claude.ai/code/session_demo" }), 1200);
        }
        return null;
      }
      const resolved: Record<string, string> = { default: "claude-opus-5-5", opus: "claude-opus-5-5", sonnet: "claude-sonnet-5-5", haiku: "claude-haiku-5-5" };
      chatSettings = {
        ...chatSettings,
        ...(args.setting.kind === "model" ? { model: resolved[args.setting.value] } : { [args.setting.kind]: args.setting.value }),
      };
      sendChat({ type: "session", ...chatSettings });
      return null;
    }
    case "session_continue_command":
      return `sbx exec -it -w ${checkout} -e IS_SANDBOX=1 worki-feat-checkout-redesign-3f2a bash -lc 'claude --resume ${args.id}'`;
    case "plugin:updater|check":
      return scene === "update"
        ? { rid: 1, currentVersion: "0.2.2", version: "0.2.3", date: null, body: null, rawJson: {} }
        : null;
    case "fetch_remote":
      return new Promise((resolve) => setTimeout(() => resolve(null), 800));
    case "list_branches":
      return [
        "main",
        "feat/checkout-redesign",
        "fix/login-race",
        "chore/react-19",
        "feat/cart-badges",
        "fix/checkout-rounding",
        "origin/main",
        "origin/feat/checkout-redesign",
        "origin/fix/login-race",
        "origin/release/2026.11",
        "origin/feat/search-filters",
        "origin/fix/checkout-tax-rounding",
      ];
    case "stop_chat_tasks":
      sendChat({ type: "backgroundTasks", tasks: [] });
      return null;
    case "interrupt_chat":
      sendChat({ type: "notice", text: "Request interrupted by user" }, { type: "turnEnded", ok: false, interrupted: true, error: null });
      return null;
    case "restore_agent_sessions":
      return { started: 0, failed: [] };
    case "restore_remote_sessions":
      return 0;
    default:
      return null;
  }
}

mockWindows("main");
mockIPC((cmd, args) => handle(cmd, (args ?? {}) as Record<string, any>), { shouldMockEvents: true });

const ui = {
  selectedProjectId: new URLSearchParams(location.search).get("project") ?? "acme-web",
  selectedWorktreePath: new URLSearchParams(location.search).has("project") ? null : checkout,
  collapsed: {},
  showChanges: scene !== "terminal",
  showTerminal: scene === "panel",
  changesScope: scene === "branch" ? "branch" : "uncommitted",
  panelWidths: { sidebar: 256, diff: 520, changes: 320 },
  theme: "dark",
  rounded: true,
  terminalFontSize: 13,
  reopenSessions: true,
  activeSession: { [checkout]: scene === "chat" || scene === "hero" ? "s5" : "s1" },
};
localStorage.setItem("worki-ui", JSON.stringify({ state: ui, version: 0 }));
localStorage.removeItem("worki-review");

await import("../main");

const { useUi } = await import("../lib/store");
if (scene === "settings") useUi.getState().openDialog({ kind: "settings", tab: "isolation" });
if (scene === "git") useUi.getState().openDialog({ kind: "settings", tab: "git" });
if (scene === "accounts") useUi.getState().openDialog({ kind: "settings", tab: "accounts" });
if (scene === "activity") {
  const open = () => {
    const button = [...document.querySelectorAll("button")].find((b) => b.title.startsWith("Start Claude Code"));
    if (button) button.click();
    else setTimeout(open, 100);
  };
  open();
}
if (scene === "isolation") {
  const open = () => {
    const badge = [...document.querySelectorAll("button")].find((b) => b.title.includes("click to change"));
    if (badge) badge.click();
    else setTimeout(open, 100);
  };
  open();
}
