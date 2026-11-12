# Statusline Usage Bridge — Design Specification

**Date:** 2026-10-27
**Status:** Revised after upstream-correctness and other-users safety reviews (see Review log). Open questions in §11 need a product-owner decision.
**Builds on:** the local-source waterfall in `src-tauri/src/poll_loop.rs` (step 1, in progress: `SnapshotOrigin::{Http, CliCache, SharedFile}`, `local_snapshot`, `adopt_local_during_bans`, full `Retry-After` handling)
**Triggered by:** `/api/oauth/usage` 429s with 1-hour bans on busy accounts. The upstream CLI already has the 5-hour and 7-day numbers for free, from the `anthropic-ratelimit-unified-*` headers on every inference response. It passes them to the statusline command on stdin and keeps them nowhere else.

**Evidence conventions.** `src/…:N` points into the upstream source snapshot at `C:\Users\xue\dev\AI\claude-code-source\claude-code\src\` (dated 2026-03-31, so it can be out of date). Quoted minified code comes from the installed binary, `@anthropic-ai/claude-code` **2.1.311** (`bin/claude.exe`, a Bun bytecode bundle — `// @bun @bytecode` — built 2026-10-26). Minified names repeat across chunks; every quoted name was resolved through its chunk's imports. The binary is the authority. Where the two disagree, this spec follows the binary. The runtime is **Bun**, not Node/libuv; any claim about process spawning, quoting or file sharing that was reasoned from Node is marked unverified.

---

## 1. Overview

This adds an opt-in setting, **"Live usage from Claude Code"**. When it is on, Switchboard points the user-level `statusLine.command` at a small **bridge** program that it installs at `~/.claude-switchboard/statusline/`. Each time the upstream CLI refreshes its status line, the bridge does three things:

1. It reads the stdin JSON.
2. It writes the `rate_limits` block to a per-process observation file, along with an observation timestamp, a data timestamp and the account identity of the config dir in use.
3. It runs the user's original statusline command, if there was one, with the same stdin, environment and cwd. The original writes straight to the bridge's inherited stdout, and its exit code is passed through. If there was no original command, the bridge prints nothing (§11 Q2).

The poll loop reads those observation files as one more local source, `SnapshotOrigin::Statusline`.

**Scope of coverage.** Claude Code runs the status line only in **interactive TUI sessions while the prompt is showing**: the component mounts only when `Ye==="prompt"&&!we.show&&!Pn&&Gr`, and on unmount the controller aborts and stops its timers. There is no status line in `-p`, SDK or headless runs, in cloud sessions, in safe mode (only the policy status line runs), in diskless mode, or in untrusted workspaces (§2.1). IDE-extension sessions are probably not covered either (unverified). For a covered session, fresh 5h/7d numbers arrive within about a second of the status line refresh that follows an API response (§2.1 cadence), and `/usage` HTTP calls for that account drop from one per polling interval to one per 15 minutes. That remaining call refreshes the Sonnet, Opus and extra-usage buckets, which the headers don't carry. Everything outside the scope keeps today's HTTP polling.

### Goals

| Goal | Decision |
|---|---|
| Works for any user on macOS and Windows, not just this machine | Yes. A self-contained native bridge with no dependency on jq, Python or a particular shell. The installed command is chosen and **proven to run** in the user's real shell by a preflight before any setting is written (§4.5) |
| Opt-in, with exact consent | Yes. Off by default. The consent text names the real settings path, the key, the bridge path and the visible side effects (§7) |
| Disabling restores the original exactly | Yes. The original `statusLine` value text is spliced back byte-for-byte (§6), as long as no other writer re-serialized the file in between |
| Uninstalling the app never breaks the user's status line | Yes, as long as `~/.claude-switchboard/statusline/` survives. The bridge lives outside the app bundle and does not need the app (§4.4). Deleting that folder by hand while enabled is the one residual case (§4.4, §11 Q4) |
| Never breaks the user's status line while installed | The bridge cannot wrap itself (§4.6), never deadlocks on output (§4.2), and startup re-runs the preflight so a bridge that stops running is surfaced with a one-click restore (§8) |
| Account-correct under swaps and multiple `CLAUDE_CONFIG_DIR`s | Yes, for sessions authenticated by the config dir's login. Every observation is tagged with an identity, passes a confirmation rule, and is cross-checked against the account's own reset times (§3, §5). Sessions authenticated by an env token cannot be identified (§3.2) |
| Fewer `/usage` calls | Yes. Active accounts with live observations drop to one HTTP call per 15 minutes |
| Long-lived-token accounts get usage at all | **Disk-stored long-lived tokens only** (the tokens Switchboard swaps into the config dir). These accounts have no `user:profile` scope, so `/usage` is unavailable (`ManagedAccount::supports_usage`, `store.rs:49-54`). The bridge becomes their only source of usage, through the `none` rule in §5.2 |

### Non-goals

- Editing project (`.claude/settings.json`) or local (`.claude/settings.local.json`) settings. Those files belong to the repository. If one of them shadows the bridge, we infer it and report it (§2.2). We never rewrite it.
- Managed or policy settings, `cowork_settings.json`, and `--settings` flag files.
- Showing our own content in the user's status line. With no original command, the bridge prints nothing. The visible side effect (Claude Code hides its footer hint, §2.1) is disclosed in the consent text; whether to print a minimal line instead is §11 Q2.
- Replacing `/usage` entirely. The headers carry only `five_hour`, `seven_day` and (for the gateway) `spend_limit`. Sonnet, Opus and extra usage still come from HTTP.
- `spend_limit` (gateway or third-party providers). v1 ignores it.
- Remote and cloud sessions. In those sessions the binary refuses to run StatusLine at all: `if(n==="StatusLine"||…)return i2(n,"unsupported_in_cloud_session")`.
- `-p`, SDK, headless and (probably) IDE-extension sessions. They get no benefit and keep HTTP polling.
- Sessions authenticated by `CLAUDE_CODE_OAUTH_TOKEN` in the env (§3.2), unless §11 Q1 decides otherwise.
- WSL and remote-host sessions. They use a different `~/.claude` that this app does not see (FAQ).
- A direct-spawn fast path that skips the second shell for simple original commands (§4.3). Deferred until the golden corpus exists.
- Linux packaging. The bridge code is portable, but the app does not ship for Linux.

---

## 2. Upstream facts this design relies on

### 2.1 How the status line command runs (2.1.311)

| Aspect | Behavior | Evidence |
|---|---|---|
| Schema | `statusLine: {type:"command", command:string, padding?:number, refreshInterval?:number≥1 (seconds), hideVimModeIndicator?:boolean}`. There is no `shell`, `timeout` or `args` field, and Zod drops unknown keys, so the `shell` and `timeout` keys that `SB` reads can never come from settings. | binary: `statusLine:()=>u({type:C("command"),command:o(),padding:v().optional(),refreshInterval:v().min(1).optional().catch(void 0)…,hideVimModeIndicator:M().optional()…})`; snapshot `src/utils/settings/types.ts:550-557` predates `refreshInterval` |
| Entry point: when it does **not** run the user `statusLine` | `QFr()` returns early when (1) **policy** `disableAllHooks` is true (`_h(){return pe("policySettings")?.disableAllHooks===!0}`); (2) **diskless mode** (`dr("statusLine")` → `a.statusLine=!0; if(a[e]&&Y())return!0`; safe mode and the minimal mode do not disable it through `dr`); (3) **workspace trust not accepted** (`zz(){return!gE()}`, "Skipping StatusLine command execution - workspace trust not accepted"), which blocks the user-level bridge too, not only project settings. Then `Kgt(e){return e0r()?pe("policySettings")?.statusLine:e}` with `e0r(){return Uy()\|\|wM()}`, `Uy(){return Hr()\|\|AZ()}`: in **safe mode** (`Hr()`: `CLAUDE_CODE_SAFE_MODE` or `--safe-mode`), under policy `allowManagedHooksOnly`, when **any merged non-policy file** (user, project, local or flag) sets `disableAllHooks: true` and policy doesn't (`AZ()`), or while an **organization ask is unanswered** (`wM()`), only the **policy** `statusLine` runs, and with none configured nothing runs. `/statusline` confirms: "safe mode only displays the managed (policy) status line". | binary `async function QFr(e,n,r,s=!1){if(_h())return;…if(dr("statusLine"))return;if(zz()){…}let y=Kgt(yr()?.statusLine);…}`; `dr`'s tables in its own chunk; `AZ(){let o=pe("policySettings");if(o?.allowManagedHooksOnly===!0)return!0;if(yr().disableAllHooks===!0&&o?.disableAllHooks!==!0)return!0;return!1}` |
| Mount scope | The status line component mounts only in prompt mode with no dialog showing: `fr=Ye==="prompt"&&!we.show&&!Pn&&Gr&&e(Yge,…)`. On unmount the controller aborts (`#s?.abort()`). | binary |
| Side effect of configuring any `statusLine` | `Gr=Jn!==void 0` (a `statusLine` is configured) → `Fr=br\|\|Gr` → `TW.FooterHintLine{…suppressHint:Fr…}`. **Configuring any status line hides Claude Code's built-in footer hint**, even when the command prints nothing. In fullscreen, an empty result still reserves a blank row (`Pn=Ko?e(zge,{text:Ko}):L?e(n,{children:" "}):null`, `L=Ti()`). | binary |
| Shell, macOS | `spawn(cmd,[],{shell:!0,detached:!0,windowsHide:!0})`, which is `/bin/sh -c`. The child gets its own process group; kill is `Z_(pid,"SIGTERM")` then `process.kill(-r,"SIGKILL")`. | binary `SB()` spawn branch: `tr=EEe(io,[],{env:ho,cwd:mr,shell:Or,detached:Go…})` with `Go=!Pt` |
| Shell, Windows | `Kt=e.shell??OG()`, `function OG(){return Rd()?"bash":"powershell"}`, `function Rd(){if(I()!=="windows")return!0;return hte()!==null}`. Git Bash lookup `x()`: `CLAUDE_CODE_GIT_BASH_PATH` (counted only if its basename is `bash.exe`, `sh.exe`, `bash` or `sh` and the file exists), then `C:\Program Files\Git\bin\bash.exe`, then `(x86)`, then `git` on PATH (cwd skipped) → `../../bin/bash.exe`. Bash branch: `EEe(io,[],{shell:<bash path>,…})`, and `ipr(ho,bashPath)` puts Git's bin directory first on `PATH`. **Otherwise PowerShell** via `EEe(<ps>, KVn(Us), …)` with `-NoProfile -NonInteractive [-ExecutionPolicy Bypass] -Command <cmd>` (`mX()`). PowerShell lookup `oy()`: `pwsh` on PATH → `%ProgramFiles%\PowerShell\7\pwsh.exe` → `%LOCALAPPDATA%\Microsoft\WindowsApps\pwsh.exe` (app-exec link) → `%USERPROFILE%\.dotnet\tools\pwsh.exe` → `powershell` on PATH → `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`. The snapshot (`hooks.ts:976`) always uses Git Bash, so the binary has changed this. | binary `function x(){…}`, `async function oy(e){…ry("pwsh",e)…Ka(s,"PowerShell","7","pwsh.exe")…NP(Ka(h,"Microsoft","WindowsApps","pwsh.exe"))…Ka(y,".dotnet","tools","pwsh.exe")…ry("powershell",e)…Ka(s,"System32","WindowsPowerShell","v1.0","powershell.exe")}`, `function mX(){…}` |
| Windows command rewrites | Bash: when the first token ends in `.sh`, it prepends `bash ` (`function apr`). PowerShell: `${CLAUDE_PROJECT_DIR}` becomes `${env:CLAUDE_PROJECT_DIR}` (`function yEr`). **PowerShell epilogues:** the first one **does** run for StatusLine whenever the command is not a bare script path (`Or=!ZUe(io)`): `xs=n==="PermissionRequest"\|\|!Or?io:io+"\n; if (-not $?) { if ($LASTEXITCODE -eq 2) { exit 2 } }"`. Only the second, longer epilogue is skipped (`Us=Or&&e.shell===void 0&&n!=="StatusLine"&&…`). Under PowerShell, exit codes other than 0 and 2 therefore surface as 1. | binary `SB()` PowerShell branch |
| Env | `ho={...(go?.inherited??{...Mo()}),...Ce,...RZe(y),CLAUDE_PROJECT_DIR:…}` plus `COLUMNS`/`LINES`. `RZe` adds `CLAUDECODE=1`, `CLAUDE_CODE_SESSION_ID`, `CLAUDE_CODE_CHILD_SESSION=1`, `CLAUDE_CODE_SESSION_ATTENDED`, `CLAUDE_PID`, and conditionally `AI_AGENT`, `CLAUDE_EFFORT`, `TRACEPARENT`. **`Mo()` always scrubs credential variables when any is present**, independent of `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB`: `gCn=["CLAUDE_CODE_OAUTH_TOKEN","CLAUDE_CODE_SESSIONS_OAUTH_TOKEN",…]`, `w=new Set(gCn.map(…))`, `Q=Object.keys(process.env).some(o=>w.has(o.toUpperCase()))\|\|…` forces the scrub path, which runs `…\|\|w.has(o.toUpperCase())&&!L(o,d)…)delete d[o]`. The only exception `L()` is a unix-socket placeholder. **So `CLAUDE_CODE_OAUTH_TOKEN` never reaches the bridge.** `CLAUDE_CONFIG_DIR` passes through. `ANTHROPIC_BASE_URL` and the provider flags (`CLAUDE_CODE_USE_BEDROCK`/`_VERTEX`/`_FOUNDRY`) are not in `gCn` or the explicit `be` list, but one name predicate (`G$e`, imported) was not resolved, so their passthrough is measurement 16. The settings `env` block (`settingsEnvForChildren`) is merged in. | binary `function Mo(e){…}`, `function L(e,n){return(e==="CLAUDE_CODE_OAUTH_TOKEN"\|\|e==="ANTHROPIC_API_KEY")&&n[e]===WCt&&n.ANTHROPIC_UNIX_SOCKET!==void 0}`, `function RZe(e){…}` (the old "scrub only when SCRUB is set" rule came from snapshot `subprocessEnv.ts:86`) |
| cwd | The session cwd, falling back to the original cwd if the cwd no longer exists. | binary `XFr(Io,S)`; snapshot `hooks.ts:930-937` |
| stdin | `tr.stdin.write(s+"\n","utf8")` followed by `end()`. That is one JSON line, then EOF. EPIPE is tolerated. | binary; snapshot `hooks.ts:1006,1210` |
| Timeout and kill | There is no 5 s statusline timeout any more. `dn=e.timeout?e.timeout*1000:xf` with `xf=600000` (600 s), and `timeout` can't come from settings. In practice a newer refresh aborts the in-flight run (`#b(){this.#s?.abort();…}`). | binary `class oHr`; snapshot `hooks.ts:4587` still shows 5 s |
| Refresh cadence | Updates are debounced by `bRr=300` ms and triggered by a new assistant message or by changes to token usage, permission mode, vim mode, model, fast mode, effort, thinking, PR status or remote session name (`ERr=[…]`). Timers: an optional `refreshInterval` (capped, `Math.min(tc,e)`), a timer at the earliest `rate_limits.*.resets_at` **+ `wRr=1000` ms**, and a timer at prompt-cache expiry (`CRr`: `prompt_cache.expires_at*1000`). A change to the command re-runs it at once. | binary `ERr`, `function ytt(e){…}`, `var bRr=300,wRr=1000`, `function CRr(e){…}`, `setInputs(){…if(n.statusLine?.command!==e.statusLine?.command)…this.#b()}` |
| Output | Only exit 0 counts. stdout is trimmed, blank lines are dropped, and the result is rendered through `<Ansi>`. Non-empty stderr is logged to debug. A non-zero exit, a spawn failure or a throw gives no status line and one telemetry event per process (`badLogged`). Claude Code itself keeps running. | binary `QFr` body |
| `rate_limits` | `{five_hour?, seven_day?, spend_limit?}`, each `{used_percentage, resets_at}`, where `used_percentage = min(round(u*1000)/10, 100)` and `resets_at` is in epoch seconds. Source is `Z8(){return ai(P.rawUtilization)}`; `ai`/`rt` already drop windows with `resets_at ≤ now` or more than a year out. `rawUtilization` lives in memory, starts empty, is never seeded from `cachedUsageUtilization`, and is **replaced wholesale** on each header reading (`applyWindowReadings(…){…if(!s){this.rawUtilization=Qe(e)…}}`). It is also updated from the headers of 429 error responses (`extractQuotaStatusFromError`) and quota probes, for which `total_api_duration_ms` may not grow. For **non-subscriber auth** it is cleared entirely (`if(!Acn(m)){this.rawUtilization={}…}`), so API-key and third-party sessions send no `rate_limits`. | binary `function Iat(e){…}`, `function Bwe(e){…}`, `let wt={...vt.five_hour&&{five_hour:…},…}`, `extractQuotaStatusFromHeaders(…)` |
| Other stdin fields | `session_id`, `transcript_path` (empty in diskless mode), `cwd`, `model`, `workspace`, `version`, `cost.{total_api_duration_ms,…}`, and, once this process has made a request, `prompt_cache.{requests,expires_at,…}` (`xRr()`; absent when `requests===0`). | binary `return{session_id:e.id,transcript_path:Y()?"":sm(e.id),…}`, `function xRr(e=Date.now()){…prompt_cache:{…requests:n.requests…}}` |
| Cost counters across processes | `--resume` keeps the `session_id` but starts a new process. Total API duration is restored only if this was the project's last session (`restoreCostStateForSession`; the binary still persists `lastAPIDuration:H$()`). Otherwise it restarts at 0. Two processes can also resume the same `session_id` at once. | snapshot `cost-tracker.ts:93-95,114,130ff`; binary `function aWe(e){let n={…lastAPIDuration:H$()…}}` |
| Settings parse | Strict: `ot` → `le(e,i){try{return{ok:!0,value:JSON.parse(Fo(e))}}catch…"safeParseJSON: invalid JSON"…}` (`Fo` strips a BOM). The bundled JSONC parser (`aUe`) is used for editor settings and keybindings, not for Claude Code settings. An empty or whitespace-only file is valid (`Ak`: `if(e.trim()==="")return{settings:{}…}`). If schema validation fails, the **whole file is ignored** (`if(!y.success)return{settings:null…}`). | binary `function Ak(e,n,r){…}` |
| Settings through symlinks | Claude Code reads settings through symlinks and watches the target too ("Settings file … is a symlink to …; also watching … so atomic-save edits to the target are detected"). | binary |

**Measured on this machine (Windows 11, profile path without spaces).** An unquoted `~/…/probe.exe` command runs correctly under Git Bash `-c`, `pwsh -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command`, Windows PowerShell 5.1 with the same flags, and `sh -c`. Spawn cost per launch from an MSYS loop: a tiny native exe took about 54 ms (MSYS fork overhead inflates this), `bash -c exit` about 130 ms, and `pwsh -Command exit` about 470 ms. This measurement does **not** cover profile paths with spaces or apostrophes, `HOME ≠ USERPROFILE`, MSYS2/Cygwin terminals, AppLocker/WDAC, or stdin delivery to a native exe under `-Command`. §4.5 replaces "measured once here" with a per-user preflight.

**Decision:** The command form is chosen per machine by the preflight (§4.5), from a fixed candidate list, and must be a single string valid in every shell Claude Code can pick on that OS. The bridge never depends on knowing which shell invoked it, except when it runs the original command (§4.3).

### 2.2 Settings precedence and shadowing

Sources merge in this order, with later ones winning: `userSettings` → `projectSettings` → `localSettings` → `flagSettings` → `policySettings` (binary `["userSettings","projectSettings","localSettings","flagSettings","policySettings"]`; snapshot `src/utils/settings/constants.ts:7-21`). The merge is a deep merge through lodash `mergeWith` (`settings.ts:87`).

- The user file is `<CLAUDE_CONFIG_DIR ?? ~/.claude>/settings.json`, or `cowork_settings.json` in cowork mode (`wT={default:"settings.json",cowork:"cowork_settings.json"}`, chosen by `coworkPlugins` or `CLAUDE_CODE_USE_COWORK_PLUGINS`). `--setting-sources` can exclude user settings entirely.
- The project file is `<projectConfigDir ?? cwd>/.claude/settings.json`. The local file comes from `qB(cwd, canonicalGitRoot)`, which for worktrees may be the main repo.
- Managed settings sit at `/Library/Application Support/ClaudeCode/managed-settings.json` on macOS and `C:\Program Files\ClaudeCode\managed-settings.json` on Windows, plus `managed-settings.d/` drop-ins, plus MDM, registry and remote sources that we cannot read reliably.

Because of the deep merge, a project file that sets `statusLine.command` replaces our command for sessions started in that directory. A project file that sets only `padding` does not. A project or local file that sets `disableAllHooks: true` switches those sessions to policy-only status lines (§2.1).

**Decision:** We always edit only the user `settings.json`. Shadowing is **inferred, not scanned**, so the app never touches project directories in the background (which on macOS would raise TCC prompts for Documents/Desktop/iCloud and can hang on SMB, `\\wsl$` or disconnected drives):

- The app already watches transcripts (`jsonl_parser/watcher.rs`) at `<config_home>/projects/<slug>/<session_id>.jsonl`. A session whose JSONL was written in the last 5 minutes but has no observation file (`obs/<session_id>.*`) is **not reporting**.
- Non-reporting sessions are grouped by project and shown as "Sessions in these projects aren't sending usage. They may set their own status line, turn off hooks, be untrusted, or run in safe mode."
- Only when the user clicks **Check** on one project does the app read that project's `.claude/settings.json` and `.claude/settings.local.json`, with a 1 s timeout, and name the exact cause (`statusLine.command` or `disableAllHooks`).
- On enable, and when the panel opens, the app reads only files it already reads without prompts: the user `settings.json` (`disableAllHooks`) and file-based managed settings (`statusLine`, `disableAllHooks`, `allowManagedHooksOnly`, including `managed-settings.d/`).

Cowork mode and `--setting-sources` cannot be seen from the app. They show up as non-reporting sessions and are named in the FAQ.

### 2.3 JSON parsing

Settings parsing is strict in 2.1.311 (§2.1 Settings parse).

**Decision:** We parse strictly with `serde_json`. A file that does not parse is refused untouched, with the reason shown. An empty or whitespace-only file is treated as `{}`, as Claude Code does, and editing it replaces it with a minimal object (recorded as "was empty" for restore). Duplicate top-level `statusLine` keys are refused (JSON.parse would take the last one; we don't guess). We preserve and re-emit a UTF-8 BOM if one is present. Before writing, we also check that our result still passes Claude Code's statusLine schema (`type:"command"`, string `command`), because a schema failure makes Claude Code ignore the whole file.

### 2.4 Account-switch staleness inside a running session

In 2.1.311, the rate-limit store carries an `accountEpoch`. `resetCurrentLimits()` clears `rawUtilization` and increments the epoch, and `isFromPreviousAccount(e){return e!==void 0&&e!==this.accountEpoch}` discards responses that started under an older epoch. `resetCurrentLimits` runs only through `k6n()`. That function is called from the login, logout and account-switch routine `Htn(…)`, which `/login` and `/logout` use, and from remote-attach teardown.

A file-based credential swap, which is what Switchboard's `swap_to` does, goes a different way. It is detected by an mtime or version check (`async function SA(e,n){…if(r!==e.lastCredentialsMtimeMs){e.lastCredentialsMtimeMs=r,qh(),Ep(e);return}…}`). That path clears token caches (`qh`) and emits `credentialsChangedOnDisk` (`Ep`). The only subscriber is `_startSiblingLoginWatch`, which adopts a sibling login only while the "please log in" notice is on screen. **A file-based swap therefore does not reset `rawUtilization`.** The old account's numbers stay in the session's status line until the first response under the new token overwrites them (wholesale, §2.1). The token read is also cached for up to 30 s (`yTn=30000`, `Date.now()-r.cachedAt<yTn`). That cache applies to the macOS keychain and to Windows Credential Manager storage. Credential Manager storage is used only behind `tengu_windows_credman` or `CLAUDE_CODE_FORCE_WINDOWS_CREDMAN=1`; plaintext-file storage on Windows has no such cache.

**Decision:** The bridge confirms observations per process (§5.3), and the app adds a check against `active_since` and a reset-time cross-check (§5.4). None of them relies on the CLI resetting state.

---

## 3. Identity: which account an observation belongs to

The stdin JSON carries no account identity, and the env carries no credential (§2.1 Env).

### 3.1 Resolution order

1. **Auth that bypasses the config dir's login.** If the env has a non-default `ANTHROPIC_BASE_URL`, or any of `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX` or `CLAUDE_CODE_USE_FOUNDRY` set, the identity is `{kind:"none", reason:"routed"}` and is never attributed. This includes Switchboard's own third-party provider switch, which writes `env.ANTHROPIC_BASE_URL` into the same `settings.json` (`providers/default_env.rs:281`), so the bridge sees it. Non-subscriber auth (API key, `apiKeyHelper`) needs no rule: Claude Code clears `rate_limits` for it (§2.1), and the observation is a heartbeat only.
2. **The config dir's global config.** The bridge finds the config home as `CLAUDE_CONFIG_DIR` from the env if it is set. If it is not set, it uses the `…/projects/…` ancestor of `transcript_path`, and failing that `~/.claude`. The global config file is chosen exactly as Claude Code's `h_()` does: legacy `<config_home>/.config.json` if it is a file that is non-empty, or writable while the new path is missing; otherwise `(CLAUDE_CONFIG_DIR || homedir)/.claude<suffix>.json`, where the suffix is the OAuth file suffix (empty for production). **`auth/paths.rs:22-37` does not match this** (it checks legacy only without `CLAUDE_CONFIG_DIR`, and under `~/.claude` rather than the config home) and is fixed alongside this work so the app and the bridge agree. The bridge deserializes only `oauthAccount.{accountUuid,emailAddress,organizationUuid}`, and serde skips everything else (a few milliseconds even on a 1–2 MB file). The identity is `{kind:"oauth", account_uuid, email, organization_uuid}`.
3. **A parsed `oauthAccount: null`** gives `{kind:"none"}`. This happens with a disk-stored long-lived token. Swapping to a token account without a profile writes `oauth_account_blob: Null` (`manager.rs:389-396`).
4. **A read or parse failure** of the global config is `ReadError`, which is **not** an identity. Claude Code rewrites `.claude.json` constantly, so partial reads and Windows sharing violations happen. The bridge retries once after 20 ms. If that also fails, it keeps the previous record's identity, writes the heartbeat, does not touch `rate_limits` or the confirmation state, and logs it.

Every observation also records `config_home` as an absolute, canonical path (§4.6). The bridge never reads credential files or the keychain: on macOS a keychain read costs about 15 ms or more and can raise an ACL prompt, and avoiding secrets keeps the bridge simple to audit.

### 3.2 Sessions using an env token cannot be identified

A session started with `CLAUDE_CODE_OAUTH_TOKEN` in its env authenticates with that token, but the variable is scrubbed before the bridge runs (§2.1 Env), and the config dir's `oauthAccount` still names whoever is signed in there. Rule 2 would credit the token account's usage to that other account, and confirmation (§5.3) can't catch it because the resolved identity never changes.

**v1 decision (pending §11 Q1):** declare it a limitation in the FAQ and the consent text, and contain the damage with the **reset-time cross-check** (§5.4 rule 5). Two accounts' 5-hour and 7-day windows almost never share the same `resets_at`, so an observation whose reset times disagree with the slot's own current HTTP snapshot is rejected and logged as "identity mismatch". This catches env tokens, proxies the rule in 3.1 misses, and any other misattribution, at the cost of rejecting data until the slot has one HTTP snapshot of the current window.

The alternative (§11 Q1) is to read the startup environment of `CLAUDE_PID` (passed by `RZe`): `KERN_PROCARGS2` on macOS, the PEB via `NtQueryInformationProcess` + `ReadProcessMemory` on Windows, and hash any `CLAUDE_CODE_OAUTH_TOKEN` found into `token:<sha256>` (`token_import.rs:22-24`). It must be measured first: Bun may change the live environment block, the token may come from the settings `env` block rather than the startup env, and reading another process's memory looks like credential access to EDR products.

**Decision:** Identity is routed-auth check → `oauthAccount` (with organization) → `none`, with `ReadError` kept separate and always tagged with `config_home`. The app matches these to managed accounts in §5.2.

---

## 4. The bridge

### 4.1 Options considered

| Option | Startup | Uninstall and update safety | Verdict |
|---|---|---|---|
| A. Headless mode of the app exe (`claude-switchboard --statusline-bridge`, branched in `main.rs` before `run()`) | The exe is a full Tauri binary (the debug build here is 43 MB). It loads WebView2Loader, WebKit and AppKit before `main`, and Defender scans large images. I estimate this at well over the ~15 ms budget, but did not measure it. | It breaks when the app moves, is uninstalled, or is translocated on macOS. On Windows, if the NSIS updater or uninstaller tries to replace the exe while a status line is running it, it can fail with a file-in-use error. The release `windows_subsystem="windows"` build would also need stdio verification. | Rejected |
| B. Shell or PowerShell script | PowerShell costs about 470 ms per refresh. Git Bash is not always present, and on Windows Claude Code may pick either shell. | Fine | Rejected. sh has no JSON parser without jq, and a script would need two implementations whose behavior drifts apart |
| C. A small separate native binary, **copied into `~/.claude-switchboard/statusline/`** | A console-subsystem binary of about 1 MB, using only `serde`, `serde_json`, `sha2`, and `windows-sys` on Windows. Target is under 15 ms excluding the original command. | It does not need the app: uninstalling or moving the app leaves the bridge working. The app re-copies it when the bundled version is newer. | **Chosen** |

**Decision:** Option C. The bridge is a new crate, `src-tauri/statusline-bridge/` (a path dependency, with lib and bin targets). The app depends on its `format` and `settings_edit` modules, so both sides share one definition of the observation and target file formats and one settings editor.

- **Build profile.** The workspace release profile sets `panic = "abort"` (`src-tauri/Cargo.toml:87`), and `panic` cannot be overridden per package. The bridge binary is built with its own profile (`[profile.bridge] inherits = "release"`, `panic = "unwind"`) so step 4's `catch_unwind` works (§4.2).
- **Packaging.** Tauri ships it as `bundle.externalBin` (`binaries/switchboard-statusline-<triple>`), so the existing bundle signing covers it. On macOS it is a **universal** binary, so a copy carried by Migration Assistant to a Mac without Rosetta still runs.
- **Install and update.** When the feature is enabled, the app copies it to `~/.claude-switchboard/statusline/bridge[.exe]`. On startup it re-copies only when the installed file is missing, fails `--version`, or reports an older semver than the bundled copy. It never downgrades, so two builds or channels sharing the folder don't flip-flop. Any bridge version must still run the `original` from a target file whose `v` it doesn't know.
- **Windows replace.** The update must not delete the running file. The app copies the new file to `bridge.new.exe`, renames `bridge.exe` to `bridge.old-<ts>.exe` (Windows allows this for a running image), then renames `bridge.new.exe` into place, retrying on a sharing violation. A launch that lands in the microseconds between the two renames fails once and the next refresh recovers. The app and the bridge both delete `bridge.old-*.exe` files they can.

### 4.2 Run-time sequence (`bridge --target <id>`)

1. **Recursion guard.** If `SWITCHBOARD_BRIDGE_DEPTH` is set, this is a nested invocation (§4.6). Write nothing, run nothing, print nothing, exit 0, and log "nested invocation".
2. Read stdin to EOF and keep all of it. Only the first 1 MB is parsed; the original always receives the full bytes.
3. Load `targets/<id>.json` (§4.4). If the file is missing or unreadable, print nothing, exit 0, and append one line to `bridge.log`. Claude Code then shows an empty status line instead of an error, and the session keeps working. Touch the target's `last_run`.
4. If the target has an original command, spawn it now (§4.3) with `SWITCHBOARD_BRIDGE_DEPTH=1` added to its env. **Its stdout is the bridge's own inherited stdout handle**, so the output is byte-exact by construction, with no copy loop and no pipe that can fill up. stderr is inherited too. The raw stdin bytes are written to its stdin from a separate thread, which ignores EPIPE and broken-pipe errors, so a child that never reads stdin can't stall the bridge. Spawning first lets the original's latency overlap with step 5.
5. Inside `catch_unwind`: parse the fields we need with `#[serde(default)]` (`session_id`, `transcript_path`, `rate_limits`, `cost.total_api_duration_ms`, `prompt_cache.requests`, `version`), read `CLAUDE_PID` from the env, resolve identity (§3), and update the observation file (§5.1). Every failure in this step, including a panic, is swallowed into `bridge.log`. Nothing here may change stdout or the exit code.
6. Wait for the child. Exit with the child's exit code; on Unix a signal death maps to `128+sig`. With no original command, exit 0.

Parse failures, unknown shapes and a missing `rate_limits` block all skip step 5's rate-limit fields. The heartbeat (`observed_at_ms`) is still written, and the record notes why there was no data, so the app can tell "the bridge runs but receives no usage" apart from "the bridge doesn't run" (§7).

**Decision:** Steps 4 and 6 pass the original through without the bridge touching its output. Steps 3 and 5 are best-effort and must never change what the user's status line shows.

### 4.3 Running the original command with the same semantics

- **macOS:** `/bin/sh -c <original>`, with the inherited env and cwd, in its own process group so a kill of the bridge's group reaches it.
- **Windows shell choice.** The bridge **replicates Claude Code's resolution** (`x()`/`Rd()` for Git Bash, `oy()` for PowerShell, §2.1). These depend only on the env and file system the bridge inherits, so the choice is deterministic. `ipr()` has already put Git's bin directory first on `PATH`, which does not change `x()`'s result. The parent process is used only as a cross-check: the bridge looks it up (`CreateToolhelp32Snapshot`, `QueryFullProcessImageNameW`), accepts it only if its creation time is earlier than the bridge's own (guarding against PID reuse), and logs a mismatch without acting on it. Git for Windows' `bin\bash.exe` is a launcher whose real child is `usr\bin\bash.exe`, so both count as bash.
- **Bash:** `<bash> -c <apr(original)>`, applying the `.sh` → `bash …` prepend.
- **PowerShell:** `<ps> -NoProfile -NonInteractive [-ExecutionPolicy Bypass unless CLAUDE_CODE_POWERSHELL_RESPECT_EXECUTION_POLICY] -Command <yEr(original) + first epilogue>`. The bridge appends the same `"\n; if (-not $?) { if ($LASTEXITCODE -eq 2) { exit 2 } }"` epilogue under the same condition Claude Code uses, so exit codes match.
- **Argument quoting.** Claude Code reaches the shell through Bun's `spawn`, not Node/libuv. The bridge builds the command line with `CommandExt::raw_arg` using a quoting routine whose output is pinned by the golden corpus (§9): about 30 hostile commands (embedded `"`, trailing `\`, `%`, `!`, `$`, backticks, newlines, non-ASCII), each run through `bun -e` with Claude Code's exact call shape (`spawn(cmd,[],{shell:<bash>})` and `spawn(<ps>,[…flags,"-Command",cmd])`) and through the bridge, asserting identical stdout and exit code. Matching Claude Code matters more than being "correct".
- **Process tree.** The child goes in a Job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so when the bridge dies the original's tree dies too. The bridge also waits on its parent's process handle and exits (closing the Job) when the parent exits, because killing the shell may not kill the bridge (unverified).
- **Console.** The child inherits the bridge's console. `CREATE_NO_WINDOW` is used only when the bridge itself has no console, so console-width queries and PowerShell output encoding match a direct run.
- On every OS, `CLAUDE_CODE_SHELL_PREFIX` is not applied again. It already wrapped the bridge.

**PowerShell is not byte-transparent.** pwsh treats a native command's stdout as text, so output that passes through Claude Code's PowerShell layer and then the bridge's is decoded and re-encoded twice. On multi-byte OEM code pages (for example cp936) powerline glyphs can change. The target is "the same output as Claude Code running the original directly", tested across PowerShell 5.1 and 7 and code pages 65001 and an OEM page (§9), not byte-exactness.

**Latency.** On a machine without Git Bash, every refresh pays PowerShell twice (about 0.5 s each). Refreshes are debounced at 300 ms during streaming and a newer refresh aborts the in-flight one, so a slow status line may rarely finish while Claude is streaming. The preflight measures the bridge's added latency, and if it exceeds 150 ms the consent text says so ("Your status line will update about 0.5 s slower"). A direct-spawn fast path for simple commands is deferred (Non-goals, §11 Q7).

**Decision:** Replicate Claude Code's shell resolution and epilogue, pin quoting with a Bun-driven golden corpus, and let the original write to our stdout directly.

### 4.4 Lifecycle and where the original command is stored

The original command is stored in three places, each for a different reason:

- **`~/.claude-switchboard/statusline/targets/<id>.json`** (mode 0600) holds `{v:1, config_home, settings_path, settings_real_path, original: <statusLine JSON value or null>, installed_command, installed_at, state, last_run, released_by?, released_at?}`. The bridge runs the original from here, so the user's status line keeps working with no app installed. Writes take an advisory lock (`fs2`, as `store.rs` already does) because two builds can share the folder; the single-instance plugin (`lib.rs:340`) covers only one identifier.
- **The app database**, in a new table `statusline_bridge_targets`, holds `(id, config_home, settings_path, settings_real_path, original_present, original_value_text (the exact bytes of the value span), inserted_member_text, file_was_created, file_was_empty, installed_command, enabled_at, state)`. This is the restore source for byte-exact disable (§6).
- **An anchor backup**, `<settings real path>.switchboard-statusline-anchor`, taken just before enable and never pruned while the target is enabled. It sits next to the settings file, so it survives the app's data folder being removed and can be restored by hand.

`<id>` is the first 12 hex characters of SHA-256 of the **canonical** settings path: symlinks and junctions resolved (`GetFinalPathNameByHandleW` on Windows, `realpath` on macOS), and case-folded on Windows. Before the file exists, the canonical parent directory plus file name is used. It is stable across our own temp+rename writes, which a file-identity id would not be.

**Order of operations.**

- **Enable:** install and verify the bridge → write and fsync the target file → take the anchor backup → run the preflight (§4.5) → edit settings. A failure at any step undoes the earlier ones and shows the error.
- **Disable:** edit settings first → mark the target `released` with `original` kept. A released target is garbage-collected only after 7 days with no `last_run`, because a running session may not reload settings at once (Claude Code's watcher waits 1–1.5 s, and sessions on network file systems or WSL `/mnt/c` may never see the change).

The bridge also has `bridge --restore [--target <id>|--all]`, which uses only the target file. It splices `original` back in when the current command is still ours, using the shared `settings_edit` code, and writes `released_by:"<caller>", released_at` into the target. Three things call it:

- The Windows NSIS uninstaller, through a `NSIS_HOOK_PREUNINSTALL` hook guarded by `${If} $UpdateMode <> 1` (Tauri v2's template parses `/UPDATE`; verify on the template version we ship). It runs `bridge.exe --restore --all --by uninstaller` when the bridge exists, then deletes `obs/`. If the restore fails, the hook leaves the bridge in place. If the uninstaller runs elevated as a different user than the one whose profile holds the bridge, it skips the restore and logs why. We keep `currentUser` install mode.
- The app's "Turn off" path, as a fallback when the app's database record is missing.
- Users, by hand. The FAQ and the consent text name this command.

**Update safety net.** If the update-mode guard ever fails, startup sees a target with `released_by:"uninstaller"` while the app is clearly running from an installed location. It re-applies the bridge automatically (the user's consent still stands), shows a quiet toast, and records it. This is the one automatic settings edit (§11 Q6).

**After uninstall on macOS**, deleting the app leaves the bridge to pass the original through for as long as the user keeps it. The app writes `~/.claude-switchboard/statusline/last_seen` on each start. If it is more than 30 days old, the bridge stops writing observations (which contain an email address) and only passes the original through.

**Residual risk.** If the user or a cleaner (AppCleaner and similar) deletes `~/.claude-switchboard/` while the setting is on, the command fails and the status line is blank until they restore. The anchor backup and `--restore` instructions in the README cover recovery. Moving the bridge under `<config_home>/switchboard/` would tie its lifetime to Claude Code's config instead, at the cost of following `~/.claude` into dotfile repos (§11 Q4).

**Decision:** The bridge never needs the app. The target file is enough to run and to restore. The app database adds byte-exact restore, and the anchor backup adds a hand-restorable copy next to the settings.

### 4.5 Preflight: proving the command runs before settings change

The command form is picked from candidates and proven in the user's real shells before any setting is written:

- **macOS:** a single-quoted absolute path, `'/Users/<u>/.claude-switchboard/statusline/bridge' --target <id>`, which only `/bin/sh` runs.
- **Windows, in order:** (1) an absolute forward-slash path when the profile path contains none of `` ␠'"$&()@{};#`% ``; (2) otherwise the 8.3 short path (`GetShortPathNameW`) when the volume has short names; (3) otherwise the tilde form `~/.claude-switchboard/statusline/bridge.exe --target <id>`. The first candidate that passes in every shell is used. If none passes, enabling is refused with the shell's stderr.

The preflight resolves the shells exactly as Claude Code does (§2.1, `x()` and `oy()`; on Windows it tests both bash and PowerShell when both exist, because installing Git later changes Claude Code's choice). For each shell it runs the exact candidate string the way Claude Code would, with sample stdin and a 10 s timeout:

1. **Probe:** against a probe target whose original is a deterministic `echo switchboard-preflight-<nonce>`. It must exit 0 and print exactly that line. This proves the command string reaches the bridge, the bridge can start (AppLocker, WDAC, Gatekeeper, noexec, CPU type) and it can spawn the shell.
2. **Real original:** through the bridge against the user's real original, compared with running the original directly. The exit codes must match. Differing stdout (clocks, git state) does not fail the preflight; both outputs are shown in the consent panel so the user can judge. The added latency is measured here (§4.3).

Startup reconciliation re-runs step 1 while any target is enabled (§8). If it fails, the target enters the **Broken** state (§7) with a one-click **Restore original** and one OS notification. A broken bridge is the one case where we prompt for an automatic restore, because the user's status line is blank because of us.

**Decision:** No settings write without a passing probe in every shell Claude Code can choose on that machine.

### 4.6 The bridge must never wrap itself

Enabling records whatever `statusLine` currently holds as the original. If that is already our command, the bridge would run itself on every refresh. This happens when the app database is lost while `settings.json` still points at the bridge, when two builds with separate databases share the folder, when "Use the bridge again" wraps a command the user or the `/statusline` agent edited into `…/bridge --target X; echo extra`, or when one settings file is reached through two paths.

- `is_bridge_command(cmd)` matches the bridge path in any spelling (tilde, absolute, forward or back slashes, 8.3 short name, with or without `.exe`, quoted or not) or `--target [0-9a-f]{12}` anywhere in the string.
- **Enable** never records a matching command as the original. If a target file already exists for the matching id, it adopts that target's `original`. If none exists, it refuses and offers to restore from the anchor backup.
- **Rewrap** ("Use the bridge again") refuses a command that contains the bridge anywhere, and says why.
- The canonical-path `<id>` (§4.4) collapses path spellings, symlinks and junctions to one target. Hardlinks are not collapsed; the recursion guard covers them.
- **Recursion guard:** the bridge sets `SWITCHBOARD_BRIDGE_DEPTH=1` for the original, and a bridge that starts with it set exits 0 silently (§4.2 step 1).

**Decision:** Three independent layers: detection at enable and rewrap, canonical ids, and a runtime depth guard.

---

## 5. Observation file and how the app consumes it

### 5.1 Format and location

`~/.claude-switchboard/statusline/obs/<session_id>.<claude_pid>.json` holds one file per CLI **process**. Two processes can share a `session_id` (`--resume` in two terminals, forks), and a resumed process restarts its counters (§2.1), so state is keyed on `(session_id, CLAUDE_PID)`. Any `session_id` outside `[A-Za-z0-9-]{1,64}` gets hashed; a missing `CLAUDE_PID` uses `0`. The file looks like this:

```json
{
  "v": 1,
  "session_id": "4b1e…",
  "claude_pid": 23140,
  "config_home": "C:/Users/xue/.claude",
  "identity": { "kind": "oauth", "account_uuid": "…", "email": "…", "organization_uuid": "…" },
  "observed_at_ms": 1761580000123,
  "api_ms": 182345,
  "requests": 41,
  "data_at_ms": 1761579998000,
  "no_data_reason": null,
  "rate_limits": {
    "five_hour": { "used_percentage": 41.3, "resets_at": 1761591600 },
    "seven_day": { "used_percentage": 12.0, "resets_at": 1762023600 }
  },
  "confirmed": true,
  "identity_since_ms": 1761570000000,
  "baseline_api_ms": 120000,
  "baseline_requests": 30,
  "growths_since_baseline": 2,
  "pre_switch_rate_limits": null,
  "cc_version": "2.1.311"
}
```

- **Writes** go to a temp file `<name>.<pid>.<rand>.tmp` in the same directory, followed by a rename. On Windows that is `MoveFileExW(REPLACE_EXISTING)`, retried three times on a sharing violation. On Unix the directory is 0700 and files are 0600, because the files contain an email address. If the existing file has a larger `observed_at_ms`, the write is skipped; this covers two overlapping runs of the same process, which happens when Claude Code aborts one run and starts the next. If the existing `observed_at_ms` is more than 5 minutes in the future, the clock jumped back, and the write proceeds.
- **Request counters.** `api_ms` is `cost.total_api_duration_ms` and `requests` is `prompt_cache.requests` (0 when absent). Data is "new" when either counter grew, or when `rate_limits` changed. A counter that goes **down** within the same file is a counter reset: re-baseline (§5.3) and treat changed `rate_limits` as new data. Header readings from 429 errors and quota probes may not grow either counter; they still count as new when `rate_limits` changed.
- **`data_at_ms`** records when the data arrived, not when the status line last ran. It becomes `now` on new data (above) or when there is no previous record, and otherwise keeps its value. A status line that refreshes every 60 s with no API traffic therefore does not make old data look fresh.
- **`no_data_reason`** is set on heartbeat-only records: `no_rate_limits` (non-subscriber auth, or no request yet), `routed` (§3.1 rule 1), `read_error`, or `parse_error`.
- **Pruning:** at most once an hour, the bridge deletes `obs/*` files whose mtime is older than 24 h, plus `*.tmp` files older than 5 minutes. It uses `obs/.pruned` as a marker. The app does the same on startup.

**Decision:** One file per process, so a stale or parallel process can never overwrite a fresh one. The app chooses the freshest confirmed file per account.

### 5.2 Matching an observation to a managed account

| Observation identity | Matches the slot whose… |
|---|---|
| `oauth{account_uuid, organization_uuid}` | `account_uuid == account_uuid`, and, when the slot has an `organization_uuid`, it equals the observation's. One person in two organizations (personal and Team) has separate limits, and the store dedupes on `account_uuid` (`store.rs:90`, `manager.rs:63`), so after a `/login` to the other org the observation is rejected rather than credited to the wrong limits. Works for active and inactive slots and for any `config_home`. |
| `none` without a reason | Only the **active** slot, and only when `config_home` is the app's own config home (`auth::paths::claude_config_home()`) and the active account is a `LongLivedToken` with a null `oauth_account_blob`. |
| `none{reason}`, `ReadError` | Nothing. |

The `token` identity of the earlier draft is gone: the env token never reaches the bridge (§3.2).

**Decision:** As in the table. Identity-less data is never attributed to an inactive slot.

### 5.3 Confirmation inside the bridge, for account switches the CLI doesn't reset

The bridge keeps state per process in the same file. When the resolved identity differs from the previous record (a `ReadError` is not a difference, §3.1), it does the following:

- It sets `identity_since_ms = now`, `confirmed = false`, baselines to `null`, `growths_since_baseline = 0` and `pre_switch_rate_limits = <previous record's rate_limits>`.
- On the first run where `now ≥ identity_since_ms + 30 s`, it freezes `baseline_api_ms` and `baseline_requests`. The 30 s matches the CLI's token cache TTL (`yTn=30000`, §2.4), which applies on macOS and on Windows only with Credential Manager storage; one rule is kept for both platforms.
- Each later run where either counter grew past its baseline increments `growths_since_baseline` and moves the baselines up.
- It sets `confirmed = true` when **either** the first growth arrives with `rate_limits ≠ pre_switch_rate_limits` (both buckets' `used_percentage` and `resets_at`), **or** a second growth arrives regardless of values. Because `rawUtilization` is replaced wholesale on every header reading (§2.1), the second response after the frozen baseline is the new account's own data even when an idle or 100%-capped account shows the same numbers.

The **first record of a process** (keyed on `CLAUDE_PID`) that already carries `rate_limits` is treated as a fresh identity change. This covers a bridge enabled mid-session after an unseen swap. A first record without `rate_limits` is the usual case, because the CLI runs the status line at mount, before any API call. In that case `confirmed = true` from the start, because a new process starts with empty `rawUtilization` under its current token. A counter reset within a file (§5.1) re-runs the baseline step without clearing `confirmed`.

**Residual risk:** A response that started under the old token before the switch and finished after the baseline was frozen could confirm the old numbers on the first-growth path. This needs a stream longer than 30 s that straddles a swap. Swaps usually happen when the account is out of quota, and then requests fail fast. The app-side checks in §5.4 (rules 3 and 5) narrow it further.

**Decision:** The app only adopts observations with `confirmed == true`.

### 5.4 Freshness rules in the app's waterfall

The poll loop adds `SnapshotOrigin::Statusline` to `local_snapshot`, next to `CliCache` and `SharedFile`, and picks the maximum by `fetched_at` exactly as step 1 does. An observation is a candidate for slot `S` only if all of the following hold:

1. `v == 1`, `confirmed`, the identity matches `S` (§5.2), and at least one of `five_hour` or `seven_day` is present.
2. `data_at` is within the step-1 `max_age` for this context: the polling interval normally, or the longer ban cut-off inside `adopt_local_during_bans`. Timestamps more than 30 s in the future are rejected.
3. If `config_home` is the app's own config home, `data_at ≥ active_since + 30 s`. This uses the existing `AppState::active_since` (`app_state.rs:146-152`) and catches A→B→A swaps that the bridge might never observe.
4. Any bucket whose `resets_at ≤ now` is dropped from the snapshot rather than shown at its old percentage. Claude Code already drops expired windows before stdin (§2.1); this covers files read later.
5. **Reset-time cross-check.** If `S` has an HTTP-origin (or `CliCache`/`SharedFile`) snapshot whose `five_hour.resets_at` or `seven_day.resets_at` is still in the future, the observation's matching `resets_at` must agree with it within a tolerance (60 s until §11's measurement confirms the two sources agree to the second). A disagreement rejects the observation and logs "identity mismatch" with both values. When the observation's window is newer (the slot's window has ended), it is accepted. Accounts without any `/usage` snapshot (disk-stored token accounts) skip this rule.

Reading is cheap. A `notify` watcher (the crate is already a dependency, used by `jsonl_parser/watcher.rs`) on `obs/`, debounced by 1 s, re-evaluates matching slots and emits `usage_updated`. The popover therefore updates within about a second of each observation. The loop tick re-scans as a backstop, caching parsed files by `(mtime, len)`.

**Config homes the app doesn't know.** A macOS app launched from Finder does not inherit `export CLAUDE_CONFIG_DIR=…` from `.zshrc`, so `claude_config_home()` (`auth/paths.rs:12-17`) can name a home the user's sessions never use. The app therefore discovers homes in use from `config_home` values in observations, from `~/.claude*` folders with recently written `projects/*/*.jsonl`, and from the target list. A home in use with no enabled target shows "Claude Code sessions are using `<path>`, which isn't connected" with **Connect** (§7).

**Notifications.** Statusline snapshots feed the existing notifier unchanged. `notifier/rules.rs` already fires once per threshold per window (`notification_last_fired`), so values that cross a threshold back and forth across sessions do not re-notify.

**Decision:** Statusline observations compete in the step-1 waterfall like any other local source, with the extra checks above.

### 5.5 Partial snapshot: merging with HTTP buckets, and what "Updated" means

An adopted observation becomes a `UsageSnapshot` built like this:

- `five_hour` and `seven_day` come from the observation. `utilization = used_percentage`; both scales run 0–100, matching `/usage` (`usage_api/types.rs:6-15`). `resets_at` is converted from epoch seconds.
- `seven_day_sonnet`, `seven_day_opus` and `extra_usage` are carried over from the slot's last HTTP snapshot. Step 1's `CliCache` and `SharedFile` origins count here too, because both are full `/usage` bodies.
- `fetched_at` is the observation's `data_at`.

`CachedUsage` gains `origin: SnapshotOrigin` (exposed to the UI) and `carried_fetched_at: Option<DateTime<Utc>>`, which is the `fetched_at` of the snapshot the carried buckets came from.

The HTTP floor works as follows. A fresh statusline adoption satisfies a slot's poll only while `carried_fetched_at` is less than **15 minutes** old (`CARRIED_BUCKET_MAX_AGE`). After that, the slot makes one HTTP fetch, which still honors backoff, so the slow buckets and the reset-time cross-check (§5.4 rule 5) stay current. This cuts `/usage` calls for busy accounts by about 15× at a 60 s interval.

Persistence: statusline-origin snapshots go to the snapshot history at most once per 60 s per account, and only when a value changed. This keeps the trend charts from getting a row every few seconds.

UI: "Updated 12s ago" in the compact popover reflects `fetched_at`, which here means the 5h/7d data. There is no new chrome in the glance view, in line with design principle 1. In the expanded details, carried buckets that are more than 2 minutes older than the headline get a muted "as of 14m ago" suffix in the existing caption token style.

**Decision:** The data is merged and the headline freshness is honest. Slow buckets are labeled with their own age, and HTTP polling continues on a 15-minute floor.

---

## 6. Editing settings.json safely

The existing `providers/default_env.rs` re-serializes the whole file. Without `serde_json`'s `preserve_order` feature (it is not enabled; `cargo tree -e features -i serde_json` shows only `default/std/alloc/raw_value`), that also sorts the keys and drops a BOM and CRLF line endings. It also uses the backup prefix `.switchboard-<ts>` with prune-to-5 (`default_env.rs:14,34-58,87-108`) and a fixed temp name `settings.switchboard-tmp` (`default_env.rs:136`).

**Decision:** Use a span splice instead of re-serializing. The bridge crate gets a small module, `settings_edit`, and both it and `default_env` take **one process-wide settings-write lock**, so the app's two writers never interleave.

1. **Resolve the file.** `lstat` it. If it is a symlink or junction, resolve it and edit the **target** in place, preserving the link; the temp file and rename go in the target's own directory, and both paths are recorded for restore. Refuse when the target is read-only, under `/nix/store`, or not writable, and say why. On Windows, refuse a file with the read-only attribute and copy the hidden and system attributes onto the new file. Explicit ACLs are not preserved (documented).
2. **Shared or synced storage.** If the resolved file has `nlink > 1`, or sits under `~/Library/Mobile Documents`, a Dropbox, OneDrive or Google Drive root, or has a `.git` ancestor, the consent panel adds a line naming the risk: "This file is shared through `<path>`. Other computers that use it will lose their status line unless Switchboard is set up there too." Enabling then requires ticking it (§11 Q3 decides whether to refuse instead).
3. **Read** the bytes, record their SHA-256, then parse strictly (§2.3). If parsing fails, refuse. If `statusLine` is present but is not an object, has `type ≠ "command"`, or has a non-string `command`, refuse and explain. An empty or whitespace-only `command` counts as no original.
4. A tiny tokenizer, about 150 lines, finds the byte span of the top-level `"statusLine"` member's value. It understands strings with escapes, nesting, a BOM, and CRLF or LF line endings, and it refuses duplicate top-level `statusLine` keys. If the value is an object, it also finds the span of its `"command"` string.
5. **Enable:**
   - Check `is_bridge_command` on the current command first (§4.6).
   - If `statusLine` exists, record `original_value_text` as the exact value span. Then replace only the `command` string token with our JSON-escaped command. `padding`, `refreshInterval`, `hideVimModeIndicator` and any other keys stay exactly as they were. For example, the owner's `{"type":"command","command":"bash ~/.claude/statusline.sh","refreshInterval":60}` keeps its `refreshInterval: 60`.
   - If `statusLine` is absent, insert the member `"statusLine": {"type": "command", "command": "…"}` after the last top-level member. Indentation is copied from the first member, and the line ending is the one the file uses. The exact inserted text is recorded.
   - If the file is empty, replace it with `{\n  "statusLine": {…}\n}\n` and record that it was empty.
   - If the file is missing, create it the same way and record that the file was created.
6. **Disable:**
   - If the current `statusLine.command` is still ours, splice `original_value_text` back into the value span. If the member was inserted, remove it and the comma we added. If the file was created (or was empty) and is still byte-identical to what we wrote, delete it (or empty it).
   - If the command is no longer ours, leave the file untouched and mark the target as released (§7). We never overwrite the user's own change.
7. **Every write:**
   - Re-parse the result and check that it is semantically equal to the input everywhere except `statusLine`, and that `statusLine` passes Claude Code's schema (§2.3). If not, abort; this guards against tokenizer bugs.
   - Back up the file to `<real path>.switchboard-statusline-<ns>` with its own prune-to-5, a prefix that `default_env`'s pruning never matches. The anchor backup (§4.4) is separate and never pruned while enabled.
   - Write a uniquely named temp file `<name>.<pid>.<rand>.tmp` in the target's directory, fsync it, and keep the file's mode on Unix.
   - Just before the rename, re-read and compare the SHA-256 (mtime and length miss same-length edits on coarse-timestamp file systems such as FAT and SMB). On a mismatch, re-read and retry up to three times, then show an error.
   - Rename over the target.
   - Claude Code's settings watcher waits for writes to settle (chokidar `awaitWriteFinish` 1000/500 ms, `src/utils/settings/changeDetector.ts:31,38`) and also watches symlink targets (§2.1), and a command change re-runs the status line at once. No restart is needed in either direction.
8. Claude Code's own settings writes, and Switchboard's provider switch (`default_env`), re-serialize the file. Our member survives them, because it is just data, but any later restore is exact at the member level, not the file level. That is the most anyone can promise once another writer has touched the file.

**Concurrent edits.** The hash check makes this optimistic concurrency. Whether rename-over succeeds on Windows while a Claude Code session holds the file open depends on how Bun opens it (share-delete), which is unverified; a manual test covers it (§9), and a failed rename surfaces as an edit error, not a silent retry loop.

---

## 7. UX

**Location:** Settings → **Polling** section, in a new Card under the interval sliders. It is a data-source setting. The toggle acts immediately through its own IPC call and is not part of the panel's batched Save, because it changes another program's file and needs its own busy, success and error feedback (memory note: surface IPC errors; `.catch(() => {})` is forbidden).

**Row:** `Toggle` labeled "Live usage from Claude Code", with the caption "Reads your 5-hour and 7-day usage from Claude Code's status line instead of polling for it."

**Consent:** turning it on for the first time runs the preflight (§4.5) with the toggle busy, then expands an inline panel in the same Card, not a modal:

> Switchboard will change one setting in `<real settings path>`: `statusLine.command` will run `<bridge path>`.
> Your current status line (`bash ~/.claude/statusline.sh`) keeps working. The bridge runs it and shows its output. *(If there was none: "Claude Code will hide its footer hint while this is on, because it now has a status line. Nothing else changes on screen.")* *(If the preflight measured more than 150 ms: "Your status line will update about 0.5 s slower.")*
> The bridge saves your usage percentages, reset times and account ID to `~/.claude-switchboard/statusline/`. Nothing leaves your computer.
> Claude Code's `/statusline` command will see the bridge command. If it changes your status line, Switchboard notices and turns live usage off.
> Live usage works in interactive Claude Code sessions. Sessions started with a `CLAUDE_CODE_OAUTH_TOKEN` environment variable, `-p` runs, and IDE sessions keep using normal polling.
> Turning this off puts the setting back exactly as it was. To undo it without Switchboard, run `<bridge path> --restore --all`.
> *(Shared file only: the sync warning from §6 step 2, with a required checkbox.)*

The buttons are **Turn on** (primary) and **Cancel**. Paths are the real ones for this machine (for example the `CLAUDE_CONFIG_DIR` path), shown in JetBrains Mono, using the numeric and mono token. When the real original's output through the bridge differed from its direct output, a disclosure shows both.

**States** (from a new `get_statusline_bridge_status` IPC call; all copy is short and uses only existing tokens and `Badge` variants):

| State | Shown as |
|---|---|
| Off | Toggle off |
| On, nothing received yet | Badge "Waiting" with the caption "Start or continue a Claude Code session." A disclosure explains what doesn't report (WSL and remote hosts, `-p`, IDE, env-token sessions). |
| Live | Badge "Live" (accent) with the caption "Last update 12s ago." Live means **usage data was adopted for a managed account** in the last 10 minutes, not merely that the bridge ran. |
| On, quiet | No badge. The caption is "Last update 2h ago." This state is normal and is not a warning. |
| Running without usage | "Claude Code is running the bridge but isn't sending usage (v2.1.311)." plus the reason when known: not signed in with a subscription, routed through a custom endpoint, account not in Switchboard, or switching accounts. Shown when heartbeats arrive in the last 10 minutes but nothing was adopted. |
| Not reporting | A warning caption, "Sessions in 3 projects aren't sending usage.", with a disclosure listing the project paths in mono and a **Check** button per project (§2.2). |
| Blocked | "Claude Code's settings turn off status line commands (disableAllHooks)." or "Your organization manages the status line." The toggle is still allowed. |
| Broken | "Your status line isn't working: the bridge can't run (<error>)." with **Restore original** (primary) and **Retry**. Raised by startup preflight (§4.5), with one OS notification. |
| Changed outside Switchboard | "Your status line was changed outside Switchboard, so live usage is off." The buttons are **Use the bridge again**, which wraps the new command as the original (refused if it contains the bridge, §4.6), and **Dismiss**. |
| Re-applied after update | A quiet toast, "Live usage was turned back on after the update." (§4.4) |
| Bridge missing or outdated | The app repairs it on startup without asking. If the repair fails: "Couldn't install the bridge: <error>" with **Retry**. |
| Unconnected config folder | "Claude Code sessions are using `<path>`, which isn't connected." with **Connect** (§5.4). |
| Claude Code not set up | The toggle is disabled, with the caption "Claude Code isn't set up on this computer." This applies when there is no config home and no global config. |
| Edit refused | The error is shown inline. Examples: "settings.json isn't valid JSON. Fix it, then try again.", "settings.json changed while Switchboard was editing it. Try again.", "settings.json is read-only.", "The status line setting has a shape Switchboard doesn't recognize.", "The bridge couldn't run in Git Bash: <stderr>". |

**Multiple config dirs:** the data model supports N targets. In v1 the UI manages the app's own config home, discovered homes (§5.4), and an "Add another Claude config folder…" link that opens a folder picker. A chosen folder must look like a config home (it has `settings.json`, `.claude.json` or `projects/`), or it is refused before anything is edited. Each folder gets its own row with the same states.

**FAQ entries:** what doesn't report and why (safe mode, untrusted workspace, diskless, cowork, `--setting-sources`, project status lines, `disableAllHooks`, WSL/remote, `-p`/SDK/IDE, env tokens); how to restore by hand; why the footer hint disappears; why `/statusline` (whose tool permission is the literal `Edit(~/.claude/settings.json)`) can't edit a `CLAUDE_CONFIG_DIR` settings file.

**Decision:** As specified. All IPC buttons show a busy state followed by success or an error message. No error is ever swallowed.

---

## 8. Backend surface

New IPC commands, all `#[command] #[specta::specta]` with typed errors:

- `get_statusline_bridge_status() -> Vec<BridgeTargetStatus>`
- `preflight_statusline_bridge(config_home: Option<String>) -> PreflightReport` (command chosen, per-shell results, latency, original output comparison, sync warning)
- `enable_statusline_bridge(config_home: Option<String>, accepted_shared_file: bool) -> BridgeTargetStatus`
- `disable_statusline_bridge(target_id: String) -> DisableOutcome { restored | released_user_changed }`
- `rewrap_statusline_bridge(target_id)`
- `restore_original_statusline(target_id)` (the Broken-state button; same path as disable)
- `check_project_statusline(project_path) -> ProjectCheck` (the on-demand **Check**, 1 s timeout)

Startup reconciliation runs when any target is enabled. It verifies the bridge binary (copying it if missing or older), verifies the target file (rewriting it from the database if missing), writes `last_seen`, re-runs the preflight probe (§4.5; failure → Broken), checks whether the settings command is still ours (setting `ChangedOutside` if not), handles `released_by:"uninstaller"` (§4.4), garbage-collects released targets older than 7 days without a run, and prunes `obs/`.

**Decision:** Settings edits happen only through explicit user actions, with one exception: re-applying after an uninstaller restore during an update (§4.4, §11 Q6). Startup otherwise only repairs our own files.

---

## 9. Test plan

**Rust unit tests, `statusline-bridge` crate, using `tempfile` dirs:**

- `settings_edit`:
  - Missing file, then create, then delete on disable. Empty file treated as `{}`, then restored to empty.
  - Absent key: insert, then remove, byte-identical.
  - Present key with `padding`, `refreshInterval` and an unknown key: only the `command` token changes.
  - BOM preserved. CRLF preserved. Tab indentation copied.
  - Unicode and escaped quotes in the original command survive a round trip.
  - Invalid JSON and JSON with comments are refused untouched (Claude Code is strict too).
  - Duplicate `statusLine` keys, a non-object `statusLine`, `type ≠ "command"` and a non-string `command` are refused; an empty `command` counts as no original.
  - The semantic-equality and schema guard trips on a deliberately bad splice.
  - Disable after the user changed `command` gives `released`, with the file untouched.
  - A hash mismatch causes a retry and then an error; a same-length, same-mtime edit is caught.
  - Symlinked `settings.json`: the link is preserved and the target edited. Read-only target refused. Hardlinked file triggers the shared-file warning.
  - Concurrent `default_env` and `settings_edit` writes serialize on the lock; `default_env` pruning never removes our backups or the anchor.
  - **Golden round trip:** for every fixture, including the symlink case, the bytes after enable then disable equal the original bytes.
- Self-wrap: `is_bridge_command` across tilde, absolute, back-slash, short-name, quoted and `.exe`-less spellings; enable twice with the database wiped in between adopts the existing target's original; rewrap of a command containing the bridge is refused; the same file through two path spellings (case, symlink, junction) gives one id; a nested call exits 0 with empty stdout.
- Identity:
  - `CLAUDE_CONFIG_DIR` env; `transcript_path` fallback.
  - `h_()` parity: legacy `.config.json` under the config home used only when non-empty, or writable while the new path is missing; `CLAUDE_CONFIG_DIR` set with a legacy file present.
  - Null `oauthAccount` gives `none`; a truncated `.claude.json` gives `ReadError` and keeps the previous identity.
  - `ANTHROPIC_BASE_URL` and each provider flag give `none{routed}`.
  - `organizationUuid` is captured.
  - A 2 MB config parses in under 10 ms (an ignored benchmark test).
- Confirmation state machine:
  - The first record of a process without `rate_limits` is confirmed; with `rate_limits`, unconfirmed.
  - Identity change → grace period → baseline frozen → growth with different values: confirmed.
  - Growth with identical values: not confirmed; a second growth: confirmed.
  - A transient `ReadError` between two good reads does not reset confirmation.
  - A new `CLAUDE_PID` for the same `session_id` gets its own file and baseline.
  - `api_ms` going down re-baselines; `prompt_cache.requests` growth alone counts as growth.
  - `data_at` advances only on new data; changed `rate_limits` with no counter growth (a 429 header reading) still counts.
  - An overlapping-run write with an older `observed_at` is skipped; a record 6 minutes in the future is overwritten.
- Pruning and the temp-file cleanup rules.

**Bridge binary integration tests** (`tests/bridge_cli.rs`): build the binary with the `bridge` profile, then run it with fake stdin and a temp `HOME` or `USERPROFILE`.

- No original: empty stdout, exit 0, and the observation file is written.
- Original `printf 'a\nb'`: stdout passes through byte-exact (macOS and Git Bash).
- Original that prints 1 MB: no deadlock, full output.
- Original that never reads stdin, with 2 MB of stdin: no stall; the original receives all bytes when it does read.
- Original `cat`: the stdin JSON is echoed, which proves stdin forwarding.
- Original that exits 3: the bridge exits 3 (bash); under PowerShell it matches Claude Code's direct result (1).
- Target file missing: empty stdout, exit 0, and a log line.
- A panic injected in step 5 (test feature): stdout and exit code unchanged.
- Windows:
  - Shell resolution matches `x()`/`Rd()`/`oy()` for: Git installed; Git absent with pwsh 7; Git absent with only Windows PowerShell 5.1; `CLAUDE_CODE_GIT_BASH_PATH` pointing at a non-bash file.
  - Under pwsh and Windows PowerShell, `cat`/`$input` echoes stdin to a native exe under `-Command`.
  - Powerline glyphs give the same output as Claude Code running the original directly, on code page 65001 and on an OEM code page (cp936), for PowerShell 5.1 and 7.
  - **Golden quoting corpus** (§4.3): about 30 hostile commands through `bun -e` with Claude Code's call shape and through the bridge give identical stdout and exit codes.
  - Killing the bridge kills a `sleep` child (the Job object); killing the parent shell ends the bridge.
- An ignored latency test asserts p50 under 15 ms for the no-original path.

**App unit tests (`poll_loop`):**

- Matching for uuid (with and without organization), and `none` (active disk-stored LongLivedToken only); `none{reason}` never matches.
- `active_since + 30 s` rejection for the app's own config home only.
- Reset-time cross-check: matching windows accepted, mismatched rejected with a log, a newer window accepted, no HTTP snapshot skips the rule.
- Buckets past their reset are dropped.
- The merge carries sonnet, opus and extra and sets `carried_fetched_at`.
- The 15-minute floor triggers an HTTP fetch.
- A statusline observation beats an older `CliCache` in `local_snapshot`.
- Live is driven by adoption; heartbeats with `no_data_reason` give "Running without usage".
- Transcript-based not-reporting inference groups sessions by project.

**Frontend (Vitest):** the toggle renders every state in §7. The consent panel shows the exact command and real paths, the footer-hint line when there is no original, the latency line, and the shared-file checkbox; Cancel writes nothing. An IPC rejection shows the error text. "Changed outside Switchboard" offers **Use the bridge again**; "Broken" offers **Restore original**.

**Manual, in the running app** (memory: build, then start the dev server, then push; the user tests in the running app):

- Enable it on this machine while the existing `bash ~/.claude/statusline.sh` status line is in place. Confirm the status line looks identical and the popover moves on each response.
- Swap accounts and confirm the numbers do not carry across.
- Run `/statusline` in Claude Code (default home only; it refuses in safe mode) to change the status line, and confirm the app reports "Changed outside Switchboard".
- With a session open, enable and disable repeatedly on Windows to confirm rename-over works while Claude Code holds the file.
- Disable it and `diff` against the anchor backup.
- **VM matrix:** profile path with a space and with an apostrophe; `HOME ≠ USERPROFILE`; Claude Code started from an MSYS2 terminal; no Git Bash (PowerShell only); a symlinked `settings.json` in a dotfiles repo; database wiped then re-enable; two terminals resuming the same session; provider env (`ANTHROPIC_BASE_URL`) plus OAuth; macOS app launched from Finder with `CLAUDE_CONFIG_DIR` in `.zshrc`; Gatekeeper on a clean Mac.
- **NSIS:** uninstall restores the status line; an in-app update does **not** turn the feature off (or, if it does, it is re-applied with a toast); an elevated uninstall by another user skips the restore.
- Disable while a session's settings watcher can't fire (settings on a network share), and confirm the released target keeps the status line working until GC.

---

## 10. Rollout and migration

- The feature ships **off**. There is no migration for existing users. On this machine, the existing `statusline-daemon.sh` and `statusline-usage.json` path (step 1's `SharedFile`) keeps working, and enabling the bridge simply wraps whatever command is configured.
- `auth/paths.rs::claude_global_config` is aligned with Claude Code's `h_()` (§3.1) in the same release, with tests, so the app and the bridge pick the same global config.
- The observation format is versioned (`v`). The app ignores versions it doesn't know. The bridge `--version` output is compared at startup to decide whether to re-copy (newer only).
- Windows: add `bundle.windows.nsis.installerHooks` with the guarded `PREUNINSTALL` restore described in §4.4.
- macOS: the bridge `externalBin` is universal and signed with the app.
- Add a CHANGELOG entry, and a README section that explains the change, what doesn't report, and gives the manual `bridge --restore --all` command and the anchor backup path.

---

## 11. Open questions

**Product-owner decisions**

1. **Env-token sessions (§3.2).** Ship v1 with the limitation plus the reset-time cross-check, or build the `CLAUDE_PID` environment read (macOS `KERN_PROCARGS2`, Windows PEB) after a measurement spike? The read gives correct attribution but costs effort and may look like credential access to EDR.
2. **No original command (§2.1 side effect).** Print nothing and disclose that Claude Code's footer hint disappears (current draft), print a minimal line (for example the model name), or decline to enable when there is no original?
3. **Shared or synced `settings.json` (§6 step 2).** Allow with an explicit extra checkbox (current draft) or refuse?
4. **Bridge location (§4.4).** Keep `~/.claude-switchboard/statusline/` (current draft; a cleaner that deletes it blanks the status line), or move it under `<config_home>/switchboard/` (survives app-data cleaners, but follows `~/.claude` into dotfile repos and onto other machines)?
5. **Custom-endpoint sessions (§3.1 rule 1).** Excluding every non-default `ANTHROPIC_BASE_URL` is conservative and also excludes harmless logging proxies that forward the user's own token. Acceptable?
6. **Automatic re-apply after an update (§4.4).** Is this one automatic settings edit acceptable, given the user's consent still stands?
7. **PowerShell-only Windows users (§4.3).** Ship with the latency disclosure, or require Git Bash, or build the direct-spawn fast path for v1?
8. **Broken-bridge OS notification (§4.5).** Acceptable for a "quiet" app, given the user's status line is blank because of us?

**Measurements before implementation**

9. **NSIS update mode:** does the Tauri v2 updater's NSIS run invoke `NSIS_HOOK_PREUNINSTALL`, and is `$UpdateMode` available in the template we ship?
10. **Reset-time agreement:** do the `anthropic-ratelimit-unified-*` reset headers and `/usage` `resets_at` agree to the second? This sets the tolerance in §5.4 rule 5.
11. **Bun spawn on Windows:** exact command-line quoting for `shell:<bash path>` and for the PowerShell args array; whether stdin reaches a native exe under `-Command`; whether Bun opens `settings.json` with share-delete.
12. **IDE extensions:** do VS Code or JetBrains sessions run the status line?
13. **macOS Gatekeeper and quarantine** on the copied, signed bridge when `/bin/sh` runs it, on a clean machine.
14. **Windows orphans:** does killing the shell leave the bridge running (the parent-wait in §4.3 assumes it might)?
15. **App exe headless startup cost and GUI-subsystem stdio:** not measured. They no longer affect the chosen option C, but they would matter if option A were ever revisited.
16. **Routing env passthrough:** confirm in a live session that `ANTHROPIC_BASE_URL` and `CLAUDE_CODE_USE_*` reach the status line command (one scrub predicate, `G$e`, was not resolved). If they are scrubbed, §3.1 rule 1 falls back to reading the user `settings.json` `env` block, and the reset-time cross-check remains the backstop.
17. **Taken from the upstream review without re-checking:** the `refreshInterval` cap (`Math.min(tc,e)`), the `--resume` cost-restore rule (snapshot only), and the `managed-settings.d/` and `qB(cwd, canonicalGitRoot)` locations.

---

## Review log

Two reviews (upstream correctness against the 2.1.311 binary; safety for other users) were checked against the binary and this repo before folding in.

**Upstream review**

| Point | Verdict | Note |
|---|---|---|
| B1 env token scrubbed, rule 1 dead | Accepted | Verified `gCn`, `Mo()` scrub loop and `L()` in the binary. Rule and `token` row removed; §3.2 added with limitation, cross-check mitigation and Q1 |
| M1 entry conditions mislabelled | Accepted | Verified `_h`, `dr` tables (diskless only for statusLine), `zz`, `Kgt`/`e0r`/`Uy`/`AZ`/`Hr`/`wM` |
| M2 configuring a status line changes the UI | Accepted | Verified `Gr`→`Fr`→`suppressHint`. Old Q1 closed; consent text fixed; choice moved to Q2 |
| M3 `api_ms` not monotonic per `session_id` | Accepted | Snapshot `cost-tracker.ts` evidence plus `lastAPIDuration` in binary. Keyed on `(session_id, CLAUDE_PID)`; `prompt_cache.requests` added (verified in `xRr`); old Q8 closed |
| M4 symlink replaced by rename | Accepted | Merged with safety B2 |
| M5 PowerShell epilogue and passthrough | Accepted | Verified both epilogues. Bridge appends the first; tests changed to "same as direct" |
| M6 coverage overclaimed | Accepted | Verified mount condition. Scope stated in §1 |
| m1 strict parse, empty file, schema failure | Accepted | Verified `le`/`Ak`. JSONC parser exists (`aUe`) but serves editor settings and keybindings, not only `bun.lock` as claimed; conclusion unchanged |
| m2 PowerShell lookup order | Accepted | Verified `oy()`. Old Q7 closed; resolution replicated, parent only a cross-check |
| m3 env row incomplete | Accepted | Verified `RZe` |
| m4 missing timers | Accepted | Verified `CRr` and `wRr=1000`; `Math.min(tc,e)` cap taken on trust |
| m5 other `rate_limits` sources, non-subscriber clearing | Accepted | Verified `extractQuotaStatusFromHeaders` clearing |
| m6 global config path order | Accepted | Verified `h_()`; `auth/paths.rs` mismatch confirmed and scheduled |
| m7 settings locations | Accepted | Cowork verified; documented, surfaced via not-reporting inference |
| m8 keychain TTL citation, credman conditional | Accepted | Verified `yTn=30000` |
| m9 Bun not libuv | Accepted | `// @bun @bytecode` confirmed; share-delete marked unverified |
| m10 `/statusline` literal path | Accepted | Verified `Edit(~/.claude/settings.json)` string |

**Safety review**

| Point | Verdict | Note |
|---|---|---|
| B1 self-wrap recursion | Accepted, partly modified | Detection, rewrap refusal, depth guard adopted. **Rejected** the `(dev, inode)`/`FileIndex` id: our own temp+rename writes change the file identity, so the id would drift; canonical resolved path used instead |
| B2 symlink, dotfiles, sync | Accepted | Edit target in place; refuse read-only/Nix; shared-file warning (Q3) |
| B3 no proof the command runs | Accepted, modified | Preflight adopted, but "stdout byte-equal to the original" would fail on any time-varying status line; a deterministic probe gates, the real-original comparison checks exit code and shows outputs |
| M1 Live without data | Accepted | Live driven by adoption; "Running without usage" state |
| M2 shadow scan TCC/hangs | Accepted | Replaced by transcript-based inference plus on-demand Check |
| M3 copy-after-exit deadlock | Accepted | stdout inherited, stdin on a thread, full stdin passed, console inherited |
| M4 Windows argument quoting | Accepted, corrected | Runtime is Bun, not Node/libuv; golden corpus driven by `bun -e` |
| M5 target lifetime and order | Accepted, partly | Order of ops and deferred GC adopted. **Rejected** mirroring the target into the config home: it doesn't help when the bridge binary itself is deleted; anchor backup used instead, location is Q4 |
| M6 uninstall hook during updates | Accepted | Guard, uninstaller marker with auto re-apply (Q6), elevated-user skip; template behavior is measurement 9 |
| M7 two processes per `session_id` | Accepted, partly | Per-PID files and counter-reset handling adopted. **Rejected** replacing the wall-clock skip with a sequence number: a read-modify-write sequence across overlapping processes is no more reliable; added a future-timestamp escape instead |
| M8 identity flapping, confirmation stuck | Accepted | `ReadError` separated; second-growth confirmation justified by the verified wholesale `rawUtilization` replacement |
| M9 misattribution via routing and orgs | Accepted, modified | Base URL and provider flags (they reach the bridge) and organization check adopted. API-key and `apiKeyHelper` rules dropped as unnecessary: non-subscriber auth already clears `rate_limits` (verified) and credential env vars are scrubbed anyway |
| M10 Finder-launched app config home | Accepted | Config-home discovery and Connect |
| M11 PowerShell double latency | Accepted, partly | Measurement and disclosure adopted; fast path deferred (Q7) |
| M12 two settings writers collide | Accepted | Verified `default_env.rs` prefix, prune-to-5 and fixed temp name. Shared lock, own prefix, anchor, unique temp |
| m1 Windows update gap | Accepted, partly | `.old` cleanup by the bridge adopted. **Rejected** `ReplaceFileW`: unverified on running images and no real atomicity gain; the gap costs one refresh |
| m2 downgrade flip-flop | Accepted | Copy only when newer |
| m3 universal macOS binary | Accepted | |
| m4 hash instead of mtime/len | Accepted | |
| m5 odd `statusLine` shapes, duplicates | Accepted | Duplicates refused |
| m6 Windows attributes | Accepted | ACLs documented as not preserved |
| m7 panic safety | Accepted, strengthened | Verified `panic = "abort"` in `Cargo.toml:87`; bridge gets its own `panic = "unwind"` profile |
| m8 Windows orphans | Accepted | Parent-wait; behavior is measurement 14 |
| m9 consent text paths and `/statusline` | Accepted | |
| m10 privacy after macOS uninstall | Accepted | `last_seen` 30-day cut-off; NSIS deletes `obs/` |
| m11 two app instances | Accepted | Advisory lock on targets |
| m12 WSL and remote | Accepted | FAQ and Waiting disclosure |
| m13 validate added folder | Accepted | |
| m14 notifier churn | Rejected | `notifier/rules.rs` already fires once per threshold per window (`notification_last_fired`), so threshold crossings don't re-notify |
| m15 test matrix gaps | Accepted | Folded into §9 |

**Added by this revision (not from either review):** the reset-time cross-check (§5.4 rule 5) as the containment for env-token sessions and any other misattribution, pending measurement 10.

---

## 12. Windows status (2026-11-12)

The Windows gate is off: the switch shows on Windows and the manager enables, disables and reconciles there.

**Verified on Windows 11** (automated, run in `cargo test`):

- The bridge binary against Git Bash, PowerShell 7 and Windows PowerShell 5.1 (`statusline-bridge/tests/bridge_cli_windows.rs`): byte-exact pass-through, 1 MB output, 2 MB stdin to an original that never reads it, the full stdin reaching the original, exit-code parity with a direct run in the same shell, stderr inheritance, the depth guard, a missing target, the 30-day `last_seen` cut-off, hostile quoting (`"`, `'`, `%`, `$`, `\`, non-ASCII) printing what a direct run prints, an original with quotes but no whitespace (`"true"`) running, and the Job object ending the original's tree when the bridge is killed.
- The manager end to end against the real bridge (`statusline_setup/windows_tests.rs`): enable, the installed command running in every shell, byte-exact disable, preflight refusal leaving no trace, startup reconcile (missing target file, missing bridge, a bridge that stops running), and replacing a bridge that is running (§4.1, the in-place replace is refused by Windows, the rename-away path is used).
- §4.5 on real shells: a command form one shell cannot run (unquoted backslashes: bash eats them) is rejected; the first form that every shell accepts is installed.

**Decisions made while implementing:**

- The probe runs in every shell Claude Code could pick; the user's real original runs once, in the shell Claude Code would pick (§4.3), because a status line written for bash fails in PowerShell with or without the bridge.
- The tilde command form is offered only for the default bridge folder.
- Command lines are written with libuv's `quote_cmd_arg` (`shell_windows::quote_arg`, via `raw_arg`), which is what Node and Bun use, not Rust's std quoting. They differ for an argument that holds a `"` but no space or tab: std leaves it bare, and Git Bash rejects `"true"` with an unterminated quote (reproduced; found in review). `quote_arg` is pinned to libuv's own documented examples. This is libuv's behaviour by reading and by those examples; Bun's own spawn still cannot be run here.
- The app's preflight and its startup probe run each command in a Job object that is closed when the run ends, and wait at most 3 s for the pipes afterwards. Git's `bin\bash.exe` is only a launcher; killing it alone left the real bash and the command alive and held the manager's lock for as long as the command ran (reproduced: 40 s for a 2 s timeout).
- The version check on a freshly written bridge gets 30 s, not 5 s. First launch of a new executable queues behind the system's scanner: 0.4-0.8 s alone, a median of 7 s with 32 launching at once (measured). This is what made four tests fail only in the full parallel run.
- Moving the bridge into place retries for about 5 s, and on a first install (nothing to move aside) it only retries, so a scanner holding the new file does not turn into "The system cannot find the file specified".
- Startup probes an installed form only if it is one of the forms the manager would install; a command the user wrapped (`<ours>; echo extra`) is probed as the first form, as macOS always did.
- The text that tells a person how to undo the edit by hand is PowerShell-runnable on Windows (`& "C:\…\bridge.exe" --restore --all`); POSIX single quotes are a parse error there.
- A test-only cargo feature, `force-shell`, lets the Windows integration tests pick the shell the bridge uses (`SWITCHBOARD_TEST_SHELL=bash|<path>` or `powershell|<path>`). It is not in release builds.

**Not verified, and not claimed:**

- The Bun-pinned quoting corpus (§4.3, §9): Bun cannot run in CI. Quoting follows libuv's routine and its documented examples instead.
- The tilde form resolves through `$HOME` in Git Bash. It is proven in the app's own environment, so a Claude Code started from an MSYS2/Cygwin terminal, or with a different exported `HOME`, can resolve it elsewhere. It is the form used when the profile path has an apostrophe, or a space on a volume without 8.3 names.
- PowerShell as the only shell: an original ending in `exit N` (N other than 0, 1, 2) exits N when run directly, but 1 through the bridge (the outer shell's epilogue), so the exact exit-code comparison refuses to enable, although Claude Code treats both as "non-zero".
- A machine with neither Git nor PowerShell 7 (Windows PowerShell 5.1 only): the bridge is tested there, the app's preflight is not.
- OEM code pages (cp936) and PowerShell's double re-encoding of non-ASCII output.
- AppLocker, WDAC, and a profile path with a space or apostrophe on a real machine (the selection rule is tested with fakes; the 8.3 short-path call is tested only where the volume has short names).
- The NSIS uninstaller hook of §4.4. It was not written: the Tauri NSIS template cannot be inspected or built here, and a wrong guard would either break the Windows release build or switch the feature off at every auto-update. Uninstalling the app leaves the bridge passing the original through; `bridge.exe --restore --all` undoes the edit; the bridge stops recording after 30 days without the app (§4.4). `obs/` is not deleted on uninstall.
- The §4.4 update safety net (re-applying after an uninstaller restore) is moot until the hook exists.
