const state = {
  user: null,
  csrfToken: null,
  presets: [],
  trackers: [],
  channels: [],
  notifications: [],
  settings: null,
  logs: [],
  rssTokens: [],
  sessions: [],
  users: [],
  eventSource: null,
};

const LEVEL_RANK = { DEBUG: 0, INFO: 1, WARN: 2, ERROR: 3 };

const STATUS_LABELS = {
  ok: "Healthy",
  running: "Checking",
  pending: "Pending",
  paused: "Paused",
  cookie_expired: "Cookie expired",
  error: "Error",
};

const CHANNEL_KIND_LABELS = {
  discord: "Discord Webhook",
  telegram: "Telegram Bot",
  gotify: "Gotify",
  ntfy: "ntfy",
};

function isAdmin() {
  return state.user && state.user.role === "admin";
}

function showToast(msg) {
  const el = document.getElementById("toast");
  el.textContent = msg;
  el.style.display = "block";
  clearTimeout(el._t);
  el._t = setTimeout(() => (el.style.display = "none"), 3000);
}

function esc(str) {
  return String(str ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function safeHref(raw) {
  if (!raw) return "";
  try {
    const parsed = new URL(String(raw).trim());
    if (parsed.protocol === "http:" || parsed.protocol === "https:") {
      return parsed.href;
    }
  } catch {}
  return "";
}

function fmtTime(iso) {
  if (!iso) return "—";
  const d = new Date(iso);
  return isNaN(d) ? iso : d.toLocaleString();
}

async function api(path, opts = {}) {
  const method = (opts.method || "GET").toUpperCase();
  const headers = { "Content-Type": "application/json", ...(opts.headers || {}) };
  if (method !== "GET" && method !== "HEAD" && state.csrfToken) {
    headers["X-CSRF-Token"] = state.csrfToken;
  }

  const res = await fetch(path, {
    credentials: "same-origin",
    ...opts,
    method,
    headers,
  });

  if (res.status === 401 && !path.startsWith("/api/auth/login") && !path.startsWith("/api/auth/setup")) {
    handleSessionExpired();
  }

  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  return data;
}

function disconnectSSE() {
  if (state.eventSource) {
    state.eventSource.close();
    state.eventSource = null;
  }
}

const MODAL_IDS = ["tracker-modal", "channel-modal", "import-modal"];
const ACCOUNT_FORMS = ["import-form", "tracker-form", "channel-form", "password-form", "rss-token-form", "create-user-form", "settings-form", "bind-form"];

// Drops everything that belongs to the signed-in account so the next user of this tab
// cannot see it (revealed RSS URL, typed secrets, cached lists, open dialogs).
function clearAccountState() {
  MODAL_IDS.forEach((id) => closeModal(id));
  resetImportForm();
  ACCOUNT_FORMS.forEach((id) => {
    const form = document.getElementById(id);
    if (form) form.reset();
  });
  const reveal = document.getElementById("rss-token-reveal");
  if (reveal) reveal.style.display = "none";
  const revealUrl = document.getElementById("rss-token-url");
  if (revealUrl) revealUrl.textContent = "";

  state.trackers = [];
  state.channels = [];
  state.notifications = [];
  state.settings = null;
  state.logs = [];
  state.rssTokens = [];
  state.sessions = [];
  state.users = [];
  state.bind = null;
}

function handleSessionExpired() {
  disconnectSSE();
  state.user = null;
  state.csrfToken = null;
  clearAccountState();
  document.getElementById("app-shell").style.display = "none";
  showLoginView();
}

function setAuthError(msg) {
  const box = document.getElementById("auth-error");
  if (!msg) {
    box.style.display = "none";
    box.textContent = "";
  } else {
    box.textContent = msg;
    box.style.display = "block";
  }
}

function showSetupView(tokenRequired) {
  document.getElementById("app-shell").style.display = "none";
  document.getElementById("auth-screen").style.display = "flex";
  document.getElementById("auth-subtitle").textContent = "Initial setup — create your administrator account";
  document.getElementById("setup-form").style.display = "block";
  document.getElementById("login-form").style.display = "none";
  const tokWrap = document.getElementById("setup-token-wrap");
  const tokInput = document.getElementById("setup-token");
  tokWrap.style.display = tokenRequired ? "flex" : "none";
  tokInput.required = Boolean(tokenRequired);
  setAuthError("");
}

function showLoginView() {
  document.getElementById("app-shell").style.display = "none";
  document.getElementById("auth-screen").style.display = "flex";
  document.getElementById("auth-subtitle").textContent = "Sign in to manage trackers and alerts";
  document.getElementById("setup-form").style.display = "none";
  document.getElementById("login-form").style.display = "block";
  setAuthError("");
  document.getElementById("login-username").focus();
}

function showAppShell() {
  document.getElementById("auth-screen").style.display = "none";
  document.getElementById("app-shell").style.display = "flex";
  const pill = document.getElementById("current-user-pill");
  if (state.user) {
    pill.textContent = `${state.user.username} (${state.user.role})`;
  }
  document.querySelectorAll(".admin-only").forEach((el) => {
    el.style.display = isAdmin() ? "" : "none";
  });
}

// Tabs
function switchTab(tabName) {
  document.querySelectorAll(".nav-btn").forEach((b) => {
    b.classList.toggle("active", b.dataset.tab === tabName);
  });
  document.querySelectorAll(".tab").forEach((t) => {
    t.classList.toggle("active", t.id === `tab-${tabName}`);
  });
  if (tabName === "security") {
    loadSecurityData().catch(() => {});
  }
}

document.querySelectorAll(".nav-btn").forEach((btn) => {
  btn.addEventListener("click", () => switchTab(btn.dataset.tab));
});

function updateNavCounts() {
  document.getElementById("nav-count-trackers").textContent = state.trackers.length;
  document.getElementById("nav-count-notifs").textContent = state.notifications.length;
  document.getElementById("nav-count-channels").textContent = state.channels.length;
}

function renderTrackers() {
  const tbody = document.getElementById("trackers-tbody");
  const empty = document.getElementById("trackers-empty");
  const total = state.trackers.length;
  const okCount = state.trackers.filter((t) => t.last_status === "ok").length;
  const errCount = state.trackers.filter(
    (t) => t.last_status === "error" || t.last_status === "cookie_expired"
  ).length;

  document.getElementById("stat-total").textContent = total;
  document.getElementById("stat-ok").textContent = okCount;
  document.getElementById("stat-err").textContent = errCount;
  document.getElementById("stat-notifs").textContent = state.notifications.length;
  updateNavCounts();

  if (total === 0) {
    tbody.innerHTML = "";
    empty.style.display = "block";
    return;
  }
  empty.style.display = "none";

  const q = (document.getElementById("filter-tracker-search").value || "").trim().toLowerCase();
  const statusFilter = document.getElementById("filter-tracker-status").value;

  const filtered = state.trackers.filter((t) => {
    if (q) {
      const hay = `${t.name} ${t.kind} ${t.base_url}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    if (statusFilter === "ok" && t.last_status !== "ok") return false;
    if (
      statusFilter === "attention" &&
      t.last_status !== "error" &&
      t.last_status !== "cookie_expired"
    ) {
      return false;
    }
    if (statusFilter === "paused" && t.enabled) return false;
    return true;
  });

  if (filtered.length === 0) {
    tbody.innerHTML = `<tr><td colspan="6" class="empty-state">No trackers match the current filter.</td></tr>`;
    return;
  }

  const admin = isAdmin();
  tbody.innerHTML = filtered
    .map((t) => {
      const statusLabel = STATUS_LABELS[t.last_status] || t.last_status;
      const actionsHtml = admin
        ? `<div class="row-gap" style="justify-content:flex-end">
             <button class="btn sm" type="button" data-action="run-tracker" data-id="${esc(t.id)}">Run now</button>
             <button class="btn sm" type="button" data-action="toggle-tracker" data-id="${esc(t.id)}" data-enabled="${!t.enabled}">${
               t.enabled ? "Pause" : "Resume"
             }</button>
             <button class="btn sm" type="button" data-action="edit-tracker" data-id="${esc(t.id)}">Edit</button>
             <button class="btn sm danger" type="button" data-action="delete-tracker" data-id="${esc(t.id)}">Delete</button>
           </div>`
        : `<span class="muted-text">Read-only</span>`;
      return `
    <tr>
      <td>
        <div class="tracker-name">${esc(t.name)}</div>
        <div class="tracker-url">${esc(t.base_url)}</div>
        ${
          t.last_error
            ? `<div class="error-tip" title="${esc(t.last_error)}">${esc(t.last_error)}</div>`
            : ""
        }
      </td>
      <td><span class="engine-tag">${esc(t.kind)}</span></td>
      <td><span class="badge ${esc(t.last_status)}">${esc(statusLabel)}</span></td>
      <td class="mono-cell">${fmtTime(t.last_checked_at)}</td>
      <td class="mono-cell">${t.enabled ? fmtTime(t.next_check_at) : "Paused"}</td>
      <td class="right">${actionsHtml}</td>
    </tr>`;
    })
    .join("");
}

function renderNotifications() {
  const filter = document.getElementById("filter-notif-type").value;
  const rssLink = document.getElementById("btn-rss-feed");
  if (rssLink) {
    rssLink.href = filter ? `/rss?item_type=${encodeURIComponent(filter)}` : "/rss";
  }
  const list = state.notifications.filter((n) => !filter || n.item_type === filter);
  const el = document.getElementById("notifications-list");
  updateNavCounts();

  if (list.length === 0) {
    el.innerHTML = `
      <div class="panel empty-state">
        <p class="empty-title">No notifications recorded yet</p>
        <p class="empty-desc">Captured notifications and private messages from active trackers will appear here.</p>
      </div>`;
    return;
  }

  el.innerHTML = list
    .map((n) => {
      const typeLabel =
        n.item_type === "message"
          ? "Private message"
          : n.item_type === "error"
            ? "Error"
            : "Notification";
      const typeClass = n.item_type === "error" ? "err" : "ok";
      const cleanBody = n.body ? n.body.replace(/<[^>]+>/g, " ").trim() : "";
      const href = safeHref(n.url);
      return `
    <article class="feed-item${n.is_staff ? " is-staff" : ""}">
      <div class="feed-meta">
        <span class="badge pending no-dot">${esc(n.tracker_name)}</span>
        <span class="badge ${typeClass} no-dot">${esc(typeLabel)}</span>
        ${n.is_staff ? `<span class="badge staff no-dot">Staff</span>` : ""}
        ${n.sender ? `<span class="feed-sender">From <strong>${esc(n.sender)}</strong></span>` : ""}
        <span class="feed-time">${esc(n.item_date || fmtTime(n.created_at))}</span>
        ${
          href
            ? `<a class="feed-link" href="${esc(href)}" target="_blank" rel="noopener noreferrer">Open</a>`
            : ""
        }
      </div>
      <div class="feed-title">${esc(n.subject || n.title || "Notification")}</div>
      ${cleanBody ? `<div class="feed-body">${esc(cleanBody)}</div>` : ""}
    </article>`;
    })
    .join("");
}

function renderChannels() {
  const el = document.getElementById("channels-grid");
  updateNavCounts();

  if (state.channels.length === 0) {
    el.innerHTML = `
      <div class="panel empty-state" style="grid-column:1/-1">
        <p class="empty-title">No channels configured yet</p>
        <p class="empty-desc">Connect Discord, Telegram, Gotify, or ntfy to receive alerts when trackers notify you.</p>
        ${
          isAdmin()
            ? `<button class="btn primary" type="button" data-action="open-channel-modal">Add channel</button>`
            : ""
        }
      </div>`;
    return;
  }

  const admin = isAdmin();
  el.innerHTML = state.channels
    .map((c) => {
      const kindLabel = CHANNEL_KIND_LABELS[c.kind] || c.kind;
      const actionsHtml = admin
        ? `<div class="channel-actions">
             <button class="btn sm" type="button" data-action="test-channel" data-id="${esc(c.id)}">Send test</button>
             <button class="btn sm" type="button" data-action="edit-channel" data-id="${esc(c.id)}">Edit</button>
             <button class="btn sm danger" type="button" data-action="delete-channel" data-id="${esc(c.id)}">Delete</button>
           </div>`
        : "";
      return `
    <div class="channel-card">
      <div class="channel-head">
        <div>
          <div class="channel-name">${esc(c.name)}</div>
          <div class="channel-kind">${esc(kindLabel)}</div>
        </div>
        <span class="badge ${c.enabled ? "ok" : "paused"}">${
          c.enabled ? "Enabled" : "Disabled"
        }</span>
      </div>
      <div class="route-pills">
        <span class="route-pill${c.notify_notifications ? "" : " is-off"}">Notifications</span>
        <span class="route-pill${c.notify_pms ? "" : " is-off"}">Private messages</span>
        <span class="route-pill${c.notify_errors ? "" : " is-off"}">Errors</span>
      </div>
      ${actionsHtml}
    </div>`;
    })
    .join("");
}

function renderLogs() {
  const minLvl = document.getElementById("filter-log-level").value;
  const minRank = minLvl === "ALL" ? -1 : LEVEL_RANK[minLvl] ?? 0;
  const box = document.getElementById("log-box");
  const filtered = state.logs.filter((l) => (LEVEL_RANK[l.level] ?? 1) >= minRank);

  if (filtered.length === 0) {
    box.innerHTML = `<div class="empty-state" style="padding:28px"><p class="empty-desc">No log entries match the selected level.</p></div>`;
    return;
  }

  box.innerHTML = filtered
    .map(
      (l) => `
    <div class="log-line">
      <span class="log-ts">${esc(l.timestamp.slice(11, 19))}</span>
      <span class="log-lvl ${esc(l.level)}">${esc(l.level)}</span>
      <span class="log-msg">${esc(l.message)}</span>
    </div>`
    )
    .join("");
  box.scrollTop = box.scrollHeight;
}

function renderSettings() {
  if (!state.settings) return;
  document.getElementById("set-interval").value = state.settings.default_check_interval_secs;
  document.getElementById("set-delay").value = state.settings.request_delay_secs;
  document.getElementById("set-timeout").value = state.settings.http_timeout_secs;
  document.getElementById("set-discord-limit").value = state.settings.discord_embed_limit;
  document.getElementById("set-mark-read").checked = state.settings.mark_as_read;
  document.getElementById("set-send-errors").checked = state.settings.send_error_notifications;
}

const BIND_SOURCE_LABELS = {
  cli: "set by --bind / PTN_BIND",
  database: "saved in the WebUI",
  default: "default",
  fallback: "fallback: the saved address could not be used",
};

function isLoopbackBind(value) {
  const v = (value || "").trim();
  return v === "" || v.startsWith("127.") || v.startsWith("[::1]:") || v.startsWith("localhost:");
}

function updateBindWarning() {
  const value = document.getElementById("bind-address").value;
  document.getElementById("bind-exposure-warning").style.display =
    state.bind && !state.bind.locked && !isLoopbackBind(value) ? "block" : "none";
}

function renderBind() {
  const b = state.bind;
  if (!b) return;
  const status = document.getElementById("bind-status");
  let text = `Listening on ${b.active} (${BIND_SOURCE_LABELS[b.source] || b.source}).`;
  if (b.restart_required) text += ` Restart PTNotifier to switch to ${b.next}.`;
  status.textContent = text;

  document.getElementById("bind-locked").style.display = b.locked ? "block" : "none";
  document.getElementById("bind-fields").style.display = b.locked ? "none" : "";
  document.getElementById("bind-actions").style.display = b.locked ? "none" : "";
  document.getElementById("bind-address").value = b.saved || b.default;
  document.getElementById("bind-password").value = "";
  updateBindWarning();
}

async function loadBind() {
  if (!isAdmin()) return;
  try {
    state.bind = await api("/api/server/bind");
    renderBind();
  } catch (err) {
    document.getElementById("bind-status").textContent = `Could not load network settings: ${err.message}`;
  }
}

async function saveBind(bind) {
  const password = document.getElementById("bind-password").value;
  if (!password) {
    showToast("Enter your password to change the listen address");
    document.getElementById("bind-password").focus();
    return;
  }
  try {
    state.bind = await api("/api/server/bind", {
      method: "PUT",
      body: JSON.stringify({ bind, password }),
    });
    renderBind();
    showToast(state.bind.restart_required ? "Saved. Restart PTNotifier to apply." : "Saved");
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
}

document.getElementById("bind-address").addEventListener("input", updateBindWarning);

document.querySelectorAll("[data-bind-preset]").forEach((btn) => {
  btn.addEventListener("click", () => {
    const input = document.getElementById("bind-address");
    const m = input.value.trim().match(/:(\d+)$/);
    const port = m ? m[1] : "8085";
    input.value = `${btn.dataset.bindPreset}:${port}`;
    updateBindWarning();
  });
});

document.getElementById("bind-form").addEventListener("submit", (e) => {
  e.preventDefault();
  saveBind(document.getElementById("bind-address").value.trim() || null);
});

document.getElementById("btn-bind-reset").addEventListener("click", () => saveBind(null));

function renderRssTokens() {
  const list = document.getElementById("rss-tokens-list");
  if (state.rssTokens.length === 0) {
    list.innerHTML = `<div class="sub-empty">No RSS tokens created yet.</div>`;
    return;
  }
  list.innerHTML = state.rssTokens
    .map(
      (t) => `
    <div class="sub-row">
      <div>
        <div class="sub-title">${esc(t.label)} <span class="mono muted-text">(${esc(t.token_prefix)}…)</span></div>
        <div class="sub-meta">Created ${fmtTime(t.created_at)} · Last used ${fmtTime(t.last_used_at)}</div>
      </div>
      <button type="button" class="btn sm danger" data-action="revoke-rss-token" data-id="${esc(t.id)}">Revoke</button>
    </div>`
    )
    .join("");
}

function renderSessions() {
  const list = document.getElementById("sessions-list");
  if (state.sessions.length === 0) {
    list.innerHTML = `<div class="sub-empty">No active sessions found.</div>`;
    return;
  }
  list.innerHTML = state.sessions
    .map(
      (s) => `
    <div class="sub-row">
      <div>
        <div class="sub-title">
          <span class="mono">${esc(s.ip_address || "unknown")}</span>
          ${s.current ? `<span class="badge ok no-dot">Current</span>` : ""}
        </div>
        <div class="sub-meta">${esc(s.user_agent)} · Last active ${fmtTime(s.last_seen_at)} · Expires ${fmtTime(s.expires_at)}</div>
      </div>
      ${
        s.current
          ? ""
          : `<button type="button" class="btn sm danger" data-action="revoke-session" data-id="${esc(s.id_prefix)}">Revoke</button>`
      }
    </div>`
    )
    .join("");
}

function renderUsers() {
  if (!isAdmin()) return;
  const list = document.getElementById("users-list");
  list.innerHTML = state.users
    .map((u) => {
      const isSelf = state.user && state.user.id === u.id;
      return `
    <div class="sub-row">
      <div>
        <div class="sub-title">
          ${esc(u.username)}
          <span class="badge ${u.role === "admin" ? "ok" : "pending"} no-dot">${esc(u.role)}</span>
          ${isSelf ? `<span class="muted-text">(you)</span>` : ""}
        </div>
        <div class="sub-meta">Created ${fmtTime(u.created_at)} · Last login ${fmtTime(u.last_login_at)}</div>
      </div>
      ${
        isSelf
          ? ""
          : `<button type="button" class="btn sm danger" data-action="delete-user" data-id="${esc(u.id)}">Delete</button>`
      }
    </div>`;
    })
    .join("");
}

async function loadSecurityData() {
  const reqs = [api("/api/auth/rss-tokens"), api("/api/auth/sessions")];
  if (isAdmin()) reqs.push(api("/api/users"));
  const [rss, sessions, users] = await Promise.all(reqs);
  state.rssTokens = rss;
  state.sessions = sessions;
  if (users) state.users = users;
  renderRssTokens();
  renderSessions();
  renderUsers();
}

// Tracker modal
function updatePresetUI() {
  const kind = document.getElementById("tr-kind").value;
  const preset = state.presets.find((p) => p.kind === kind);
  if (!preset) return;
  const urlInput = document.getElementById("tr-url");
  if (!document.getElementById("tr-id").value || !urlInput.value) {
    urlInput.value = preset.default_base_url;
  }
  const notes = document.getElementById("tr-preset-notes");
  if (preset.notes) {
    notes.textContent = preset.notes;
    notes.style.display = "block";
  } else {
    notes.style.display = "none";
  }
  document.getElementById("tr-cookies-wrap").style.display =
    preset.auth === "api_token" ? "none" : "flex";
  document.getElementById("tr-token-wrap").style.display =
    preset.auth === "cookies" ? "none" : "flex";
}

function openTrackerModal(id = null) {
  const modal = document.getElementById("tracker-modal");
  document.getElementById("tracker-form").reset();
  document.getElementById("tr-id").value = id || "";
  document.getElementById("tracker-modal-title").textContent = id ? "Edit tracker" : "Add tracker";

  const cookiesInput = document.getElementById("tr-cookies");
  const tokenInput = document.getElementById("tr-token");
  const clearCookiesWrap = document.getElementById("tr-clear-cookies-wrap");
  const clearTokenWrap = document.getElementById("tr-clear-token-wrap");

  cookiesInput.placeholder = "# Netscape HTTP Cookie File... or Cookie: uid=123; pass=abc";
  tokenInput.placeholder = "API key (for Anthelion / Orpheus)";
  clearCookiesWrap.style.display = "none";
  clearTokenWrap.style.display = "none";

  if (id) {
    const t = state.trackers.find((x) => x.id === id);
    if (t) {
      document.getElementById("tr-kind").value = t.kind;
      document.getElementById("tr-name").value = t.name;
      document.getElementById("tr-url").value = t.base_url;
      document.getElementById("tr-interval").value = t.check_interval_secs ?? "";
      document.getElementById("tr-mark-read").value =
        t.mark_as_read === null || t.mark_as_read === undefined ? "" : String(t.mark_as_read);
      document.getElementById("tr-ignore").value = (t.ignore_patterns || []).join("\n");
      document.getElementById("tr-enabled").checked = t.enabled;

      if (t.cookies_configured) {
        cookiesInput.placeholder = "Saved (leave blank to keep existing cookies)";
        clearCookiesWrap.style.display = "inline-flex";
      }
      if (t.api_token_configured) {
        tokenInput.placeholder = "Saved (leave blank to keep existing API token)";
        clearTokenWrap.style.display = "inline-flex";
      }
    }
  }
  updatePresetUI();
  showModal("tracker-modal");
}

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
const modalOpeners = {};

function modalFocusables(modal) {
  return [...modal.querySelectorAll(FOCUSABLE)].filter((el) => el.offsetParent !== null);
}

function isModalOpen(id) {
  return document.getElementById(id).style.display === "flex";
}

// Shows a modal, moves focus into it and remembers what to give focus back to.
function showModal(id) {
  const modal = document.getElementById(id);
  if (!isModalOpen(id)) modalOpeners[id] = document.activeElement;
  modal.style.display = "flex";
  const first = modalFocusables(modal)[0];
  if (first) first.focus();
}

function closeModal(id) {
  const wasOpen = isModalOpen(id);
  document.getElementById(id).style.display = "none";
  const opener = modalOpeners[id];
  delete modalOpeners[id];
  if (wasOpen && opener && document.contains(opener) && typeof opener.focus === "function") {
    opener.focus();
  }
}

document.getElementById("tr-kind").addEventListener("change", () => {
  if (!document.getElementById("tr-id").value) {
    const preset = state.presets.find((p) => p.kind === document.getElementById("tr-kind").value);
    if (preset) {
      document.getElementById("tr-url").value = preset.default_base_url;
      if (!document.getElementById("tr-name").value) {
        document.getElementById("tr-name").value = preset.label.split(" (")[0];
      }
    }
  }
  updatePresetUI();
});

document.getElementById("tr-cookie-file").addEventListener("change", async (e) => {
  const file = e.target.files?.[0];
  if (!file) return;
  document.getElementById("tr-cookies").value = await file.text();
});

document.getElementById("btn-add-tracker").addEventListener("click", () => openTrackerModal());

// ---------------------------------------------------------------------------
// Bulk cookie import
// ---------------------------------------------------------------------------
const IMPORT_MAX_FILES = 100;
const IMPORT_MAX_FILE_BYTES = 64 * 1024;
const IMPORT_STATUS_BADGE = { created: "ok", updated: "running", skipped: "paused", error: "error" };

function importSelectedFiles() {
  const picked = [
    ...document.getElementById("imp-files").files,
    ...document.getElementById("imp-folder").files,
  ];
  // Folders can contain anything; keep the text files.
  return picked.filter((f) => /\.txt$/i.test(f.name));
}

function updateImportSelection() {
  const files = importSelectedFiles();
  const el = document.getElementById("imp-selection");
  if (files.length === 0) el.textContent = "No .txt files selected.";
  else if (files.length > IMPORT_MAX_FILES) el.textContent = `${files.length} files selected — at most ${IMPORT_MAX_FILES} per import.`;
  else el.textContent = `${files.length} file(s) selected.`;
}

function resetImportForm() {
  document.getElementById("import-form").reset();
  document.getElementById("imp-results").innerHTML = "";
  updateImportSelection();
}

function openImportModal() {
  const sel = document.getElementById("imp-kind");
  const cookiePresets = state.presets.filter((p) => p.auth !== "api_token");
  sel.innerHTML =
    `<option value="">Auto-detect from cookies</option>` +
    cookiePresets.map((p) => `<option value="${esc(p.kind)}">${esc(p.label)}</option>`).join("");
  resetImportForm();
  showModal("import-modal");
}

function renderImportResults(results) {
  document.getElementById("imp-results").innerHTML = results
    .map(
      (r) => `
    <div class="sub-row">
      <div>
        <div class="sub-title"><span class="mono">${esc(r.file)}</span></div>
        <div class="sub-meta">${esc(r.message)}</div>
      </div>
      <span class="badge ${IMPORT_STATUS_BADGE[r.status] || ""}">${esc(r.status)}</span>
    </div>`
    )
    .join("");
}

document.getElementById("btn-import-cookies").addEventListener("click", openImportModal);
document.getElementById("imp-files").addEventListener("change", updateImportSelection);
document.getElementById("imp-folder").addEventListener("change", updateImportSelection);

document.getElementById("import-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const files = importSelectedFiles();
  if (files.length === 0) {
    showToast("Choose at least one cookies .txt file");
    return;
  }
  if (files.length > IMPORT_MAX_FILES) {
    showToast(`At most ${IMPORT_MAX_FILES} files per import`);
    return;
  }
  const submit = document.getElementById("imp-submit");
  submit.disabled = true;
  try {
    const tooLarge = [];
    const payloadFiles = [];
    for (const f of files) {
      const name = f.webkitRelativePath || f.name;
      if (f.size > IMPORT_MAX_FILE_BYTES) tooLarge.push({ file: name, status: "error", message: "File is too large for a cookie file" });
      else payloadFiles.push({ name, content: await f.text() });
    }
    let results = tooLarge;
    if (payloadFiles.length) {
      const res = await api("/api/trackers/import", {
        method: "POST",
        body: JSON.stringify({
          kind: document.getElementById("imp-kind").value || null,
          update_existing: document.getElementById("imp-update").checked,
          files: payloadFiles,
        }),
      });
      results = res.results.concat(tooLarge);
    }
    renderImportResults(results);
    const created = results.filter((r) => r.status === "created").length;
    const updated = results.filter((r) => r.status === "updated").length;
    const failed = results.filter((r) => r.status === "error").length;
    showToast(`Import: ${created} created, ${updated} updated, ${failed} failed`);
    if (created || updated) await loadAll();
  } catch (err) {
    showToast(`Import failed: ${err.message}`);
  } finally {
    submit.disabled = false;
  }
});
document.getElementById("btn-empty-add-tracker").addEventListener("click", () => openTrackerModal());

document.getElementById("tracker-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const id = document.getElementById("tr-id").value;
  const intervalVal = document.getElementById("tr-interval").value;
  const marVal = document.getElementById("tr-mark-read").value;
  const ignore = document
    .getElementById("tr-ignore")
    .value.split("\n")
    .map((s) => s.trim())
    .filter(Boolean);

  const cookiesVal = document.getElementById("tr-cookies").value;
  const tokenVal = document.getElementById("tr-token").value;

  const payload = {
    name: document.getElementById("tr-name").value,
    kind: document.getElementById("tr-kind").value,
    base_url: document.getElementById("tr-url").value,
    enabled: document.getElementById("tr-enabled").checked,
    check_interval_secs: intervalVal ? Number(intervalVal) : null,
    mark_as_read: marVal === "" ? null : marVal === "true",
    ignore_patterns: ignore,
  };

  if (id) {
    payload.clear_cookies = document.getElementById("tr-clear-cookies").checked;
    payload.clear_api_token = document.getElementById("tr-clear-token").checked;
    if (cookiesVal.trim()) payload.cookies_raw = cookiesVal;
    if (tokenVal.trim()) payload.api_token = tokenVal;
  } else {
    payload.cookies_raw = cookiesVal;
    payload.api_token = tokenVal;
  }

  try {
    if (id) {
      await api(`/api/trackers/${id}`, { method: "PUT", body: JSON.stringify(payload) });
      showToast("Tracker updated");
    } else {
      await api("/api/trackers", { method: "POST", body: JSON.stringify(payload) });
      showToast("Tracker added");
    }
    closeModal("tracker-modal");
    await loadAll();
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
});

async function runTracker(id) {
  try {
    await api(`/api/trackers/${id}/run`, { method: "POST" });
    showToast("Check queued");
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
}

async function toggleTracker(id, enabled) {
  try {
    await api(`/api/trackers/${id}`, { method: "PUT", body: JSON.stringify({ enabled }) });
    await loadAll();
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
}

async function deleteTracker(id) {
  if (!confirm("Delete this tracker?")) return;
  try {
    await api(`/api/trackers/${id}`, { method: "DELETE" });
    showToast("Tracker deleted");
    await loadAll();
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
}

// Channel modal
function renderChannelFields(kind, cfg = {}, secretFields = []) {
  const wrap = document.getElementById("ch-fields");
  const hasSecret = (k) => secretFields.includes(k);
  const ph = (k, fallback) => (hasSecret(k) ? "Saved (leave blank to keep existing)" : fallback);
  const req = (k) => (hasSecret(k) ? "" : "required");

  if (kind === "discord") {
    wrap.innerHTML = `<label class="field full"><span class="field-label">Webhook URL</span><input type="url" class="input mono" id="ch-cfg-webhook" value="${esc(cfg.webhook_url || "")}" placeholder="${esc(ph("webhook_url", "https://discord.com/api/webhooks/..."))}" ${req("webhook_url")} /></label>`;
  } else if (kind === "telegram") {
    wrap.innerHTML = `
      <label class="field full"><span class="field-label">Bot token</span><input type="text" class="input mono" id="ch-cfg-bot" value="${esc(cfg.bot_token || "")}" placeholder="${esc(ph("bot_token", "123456:ABC-DEF..."))}" ${req("bot_token")} /></label>
      <label class="field"><span class="field-label">Chat ID</span><input type="text" class="input mono" id="ch-cfg-chat" value="${esc(cfg.chat_id || "")}" required /></label>
      <label class="field"><span class="field-label">Topic ID (optional)</span><input type="text" class="input mono" id="ch-cfg-topic" value="${esc(cfg.topic_id || "")}" /></label>`;
  } else if (kind === "gotify") {
    wrap.innerHTML = `
      <label class="field"><span class="field-label">Gotify URL</span><input type="url" class="input mono" id="ch-cfg-url" value="${esc(cfg.url || "")}" required /></label>
      <label class="field"><span class="field-label">App token</span><input type="text" class="input mono" id="ch-cfg-token" value="${esc(cfg.token || "")}" placeholder="${esc(ph("token", "App token"))}" ${req("token")} /></label>`;
  } else if (kind === "ntfy") {
    wrap.innerHTML = `
      <label class="field"><span class="field-label">ntfy URL</span><input type="url" class="input mono" id="ch-cfg-url" value="${esc(cfg.url || "https://ntfy.sh")}" required /></label>
      <label class="field"><span class="field-label">Topic</span><input type="text" class="input mono" id="ch-cfg-topic" value="${esc(cfg.topic || "")}" required /></label>
      <label class="field"><span class="field-label">Access token (optional)</span><input type="text" class="input mono" id="ch-cfg-token" value="${esc(cfg.token || "")}" placeholder="${esc(ph("token", "Optional bearer token"))}" /></label>
      <label class="field"><span class="field-label">Priority (1-5)</span><input type="number" class="input mono" id="ch-cfg-prio" min="1" max="5" value="${esc(cfg.priority ?? 3)}" /></label>`;
  }
}

function openChannelModal(id = null) {
  document.getElementById("channel-form").reset();
  document.getElementById("ch-id").value = id || "";
  document.getElementById("channel-modal-title").textContent = id ? "Edit channel" : "Add channel";
  const kindSel = document.getElementById("ch-kind");
  kindSel.disabled = Boolean(id);

  if (id) {
    const c = state.channels.find((x) => x.id === id);
    if (c) {
      document.getElementById("ch-name").value = c.name;
      kindSel.value = c.kind;
      document.getElementById("ch-enabled").checked = c.enabled;
      document.getElementById("ch-notifs").checked = c.notify_notifications;
      document.getElementById("ch-pms").checked = c.notify_pms;
      document.getElementById("ch-errors").checked = c.notify_errors;
      renderChannelFields(c.kind, c.config, c.secret_fields || []);
    }
  } else {
    renderChannelFields(kindSel.value);
  }
  showModal("channel-modal");
}

document.getElementById("ch-kind").addEventListener("change", (e) => renderChannelFields(e.target.value));
document.getElementById("btn-add-channel").addEventListener("click", () => openChannelModal());

document.getElementById("channel-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const id = document.getElementById("ch-id").value;
  const kind = document.getElementById("ch-kind").value;
  let config = {};
  if (kind === "discord") {
    config = { webhook_url: document.getElementById("ch-cfg-webhook").value };
  } else if (kind === "telegram") {
    config = {
      bot_token: document.getElementById("ch-cfg-bot").value,
      chat_id: document.getElementById("ch-cfg-chat").value,
      topic_id: document.getElementById("ch-cfg-topic").value,
    };
  } else if (kind === "gotify") {
    config = {
      url: document.getElementById("ch-cfg-url").value,
      token: document.getElementById("ch-cfg-token").value,
    };
  } else if (kind === "ntfy") {
    config = {
      url: document.getElementById("ch-cfg-url").value,
      topic: document.getElementById("ch-cfg-topic").value,
      token: document.getElementById("ch-cfg-token").value,
      priority: Number(document.getElementById("ch-cfg-prio").value || 3),
    };
  }

  const payload = {
    name: document.getElementById("ch-name").value,
    kind,
    enabled: document.getElementById("ch-enabled").checked,
    config,
    notify_notifications: document.getElementById("ch-notifs").checked,
    notify_pms: document.getElementById("ch-pms").checked,
    notify_errors: document.getElementById("ch-errors").checked,
  };

  try {
    if (id) {
      await api(`/api/channels/${id}`, { method: "PUT", body: JSON.stringify(payload) });
      showToast("Channel updated");
    } else {
      await api("/api/channels", { method: "POST", body: JSON.stringify(payload) });
      showToast("Channel added");
    }
    closeModal("channel-modal");
    await loadAll();
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
});

async function testChannel(id) {
  try {
    await api(`/api/channels/${id}/test`, { method: "POST" });
    showToast("Test notification sent");
  } catch (err) {
    showToast(`Test failed: ${err.message}`);
  }
}

async function deleteChannel(id) {
  if (!confirm("Delete this channel?")) return;
  try {
    await api(`/api/channels/${id}`, { method: "DELETE" });
    showToast("Channel deleted");
    await loadAll();
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
}

document.getElementById("settings-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const payload = {
    default_check_interval_secs: Number(document.getElementById("set-interval").value),
    request_delay_secs: Number(document.getElementById("set-delay").value),
    http_timeout_secs: Number(document.getElementById("set-timeout").value),
    discord_embed_limit: Number(document.getElementById("set-discord-limit").value),
    mark_as_read: document.getElementById("set-mark-read").checked,
    send_error_notifications: document.getElementById("set-send-errors").checked,
  };
  try {
    state.settings = await api("/api/settings", { method: "PUT", body: JSON.stringify(payload) });
    showToast("Settings saved");
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
});

// Auth forms & security handlers
document.getElementById("setup-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  setAuthError("");
  const pw = document.getElementById("setup-password").value;
  const confirmPw = document.getElementById("setup-password-confirm").value;
  if (pw !== confirmPw) {
    setAuthError("Passwords do not match");
    return;
  }
  const payload = {
    username: document.getElementById("setup-username").value,
    password: pw,
    setup_token: document.getElementById("setup-token").value || null,
  };
  try {
    const res = await api("/api/auth/setup", { method: "POST", body: JSON.stringify(payload) });
    state.user = res.user;
    state.csrfToken = res.csrf_token;
    document.getElementById("setup-form").reset();
    showAppShell();
    await loadAll();
    connectSSE();
  } catch (err) {
    setAuthError(err.message);
  }
});

document.getElementById("login-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  setAuthError("");
  const payload = {
    username: document.getElementById("login-username").value,
    password: document.getElementById("login-password").value,
  };
  try {
    const res = await api("/api/auth/login", { method: "POST", body: JSON.stringify(payload) });
    state.user = res.user;
    state.csrfToken = res.csrf_token;
    document.getElementById("login-form").reset();
    showAppShell();
    await loadAll();
    connectSSE();
  } catch (err) {
    setAuthError(err.message);
  }
});

document.getElementById("btn-logout").addEventListener("click", async () => {
  try {
    await api("/api/auth/logout", { method: "POST" });
  } catch (err) {
    // A 401 already signed this tab out (inside api()). Anything else means the server may
    // still hold the session, so do not pretend the browser is signed out.
    if (state.user) {
      showToast(`Sign out failed: ${err.message}. Click "Sign out" to try again.`);
      return;
    }
  }
  handleSessionExpired();
});

document.getElementById("password-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const current_password = document.getElementById("pw-current").value;
  const new_password = document.getElementById("pw-new").value;
  const confirm_password = document.getElementById("pw-confirm").value;
  if (new_password !== confirm_password) {
    showToast("New passwords do not match");
    return;
  }
  try {
    await api("/api/auth/password", {
      method: "POST",
      body: JSON.stringify({ current_password, new_password }),
    });
    document.getElementById("password-form").reset();
    showToast("Password updated and other sessions signed out");
    await loadSecurityData();
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
});

document.getElementById("rss-token-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const label = document.getElementById("rss-token-label").value;
  try {
    const res = await api("/api/auth/rss-tokens", {
      method: "POST",
      body: JSON.stringify({ label }),
    });
    const fullUrl = `${window.location.origin}/rss?token=${encodeURIComponent(res.token)}`;
    document.getElementById("rss-token-url").textContent = fullUrl;
    document.getElementById("rss-token-reveal").style.display = "block";
    document.getElementById("rss-token-label").value = "";
    showToast("RSS token generated");
    await loadSecurityData();
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
});

document.getElementById("btn-copy-rss-url").addEventListener("click", async () => {
  const url = document.getElementById("rss-token-url").textContent;
  try {
    await navigator.clipboard.writeText(url);
    showToast("RSS URL copied to clipboard");
  } catch {
    showToast("Copy failed — copy the URL manually");
  }
});

document.getElementById("btn-revoke-other-sessions").addEventListener("click", async () => {
  try {
    await api("/api/auth/sessions/others", { method: "POST" });
    showToast("Other sessions revoked");
    await loadSecurityData();
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
});

document.getElementById("create-user-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const username = document.getElementById("new-user-name").value;
  const password = document.getElementById("new-user-pw").value;
  const role = document.getElementById("new-user-role").value;
  try {
    await api("/api/users", {
      method: "POST",
      body: JSON.stringify({ username, password, role }),
    });
    document.getElementById("create-user-form").reset();
    showToast("User created");
    await loadSecurityData();
  } catch (err) {
    showToast(`Error: ${err.message}`);
  }
});

// Delegated click handlers (no inline onclick needed under strict CSP)
document.addEventListener("click", async (e) => {
  const closeBtn = e.target.closest("[data-close-modal]");
  if (closeBtn) {
    closeModal(closeBtn.dataset.closeModal);
    return;
  }

  const btn = e.target.closest("[data-action]");
  if (!btn) return;
  const action = btn.dataset.action;
  const id = btn.dataset.id;

  if (action === "run-tracker") await runTracker(id);
  else if (action === "toggle-tracker") await toggleTracker(id, btn.dataset.enabled === "true");
  else if (action === "edit-tracker") openTrackerModal(id);
  else if (action === "delete-tracker") await deleteTracker(id);
  else if (action === "open-channel-modal") openChannelModal();
  else if (action === "test-channel") await testChannel(id);
  else if (action === "edit-channel") openChannelModal(id);
  else if (action === "delete-channel") await deleteChannel(id);
  else if (action === "revoke-rss-token") {
    try {
      await api(`/api/auth/rss-tokens/${id}`, { method: "DELETE" });
      showToast("RSS token revoked");
      await loadSecurityData();
    } catch (err) {
      showToast(`Error: ${err.message}`);
    }
  } else if (action === "revoke-session") {
    try {
      await api(`/api/auth/sessions/${id}`, { method: "DELETE" });
      showToast("Session revoked");
      await loadSecurityData();
    } catch (err) {
      showToast(`Error: ${err.message}`);
    }
  } else if (action === "delete-user") {
    if (!confirm("Delete this user account?")) return;
    try {
      await api(`/api/users/${id}`, { method: "DELETE" });
      showToast("User deleted");
      await loadSecurityData();
    } catch (err) {
      showToast(`Error: ${err.message}`);
    }
  }
});

// Close modals on Escape or backdrop click
document.addEventListener("keydown", (e) => {
  const openId = MODAL_IDS.find(isModalOpen);
  if (!openId) return;
  if (e.key === "Escape") {
    closeModal(openId);
    return;
  }
  if (e.key !== "Tab") return;
  // Keep keyboard focus inside the open dialog.
  const modal = document.getElementById(openId);
  const items = modalFocusables(modal);
  if (items.length === 0) {
    e.preventDefault();
    return;
  }
  const first = items[0];
  const last = items[items.length - 1];
  if (!modal.contains(document.activeElement)) {
    e.preventDefault();
    first.focus();
  } else if (e.shiftKey && document.activeElement === first) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && document.activeElement === last) {
    e.preventDefault();
    first.focus();
  }
});

MODAL_IDS.forEach((id) => {
  document.getElementById(id).addEventListener("click", (e) => {
    if (e.target.id === id) closeModal(id);
  });
});

document.getElementById("filter-tracker-search").addEventListener("input", renderTrackers);
document.getElementById("filter-tracker-status").addEventListener("change", renderTrackers);
document.getElementById("filter-notif-type").addEventListener("change", renderNotifications);
document.getElementById("btn-refresh-notifs").addEventListener("click", loadAll);
document.getElementById("filter-log-level").addEventListener("change", renderLogs);
document.getElementById("btn-clear-logs").addEventListener("click", () => {
  state.logs = [];
  renderLogs();
});

// SSE events that arrive while a snapshot is loading would be overwritten by it, so they
// are buffered and replayed on top of the snapshot.
let snapshotLoads = 0;
let bufferedEvents = [];

async function loadAll() {
  snapshotLoads += 1;
  try {
    await loadSnapshot();
  } finally {
    snapshotLoads -= 1;
    if (snapshotLoads === 0 && bufferedEvents.length) {
      const pending = bufferedEvents;
      bufferedEvents = [];
      pending.forEach((ev) => applyServerEvent(ev, true));
    }
  }
}

async function loadSnapshot() {
  const [presets, ov, logs] = await Promise.all([
    api("/api/trackers/presets"),
    api("/api/overview"),
    api("/api/logs"),
  ]);
  state.presets = presets;
  state.settings = ov.settings;
  state.trackers = ov.trackers;
  state.channels = ov.channels;
  state.notifications = ov.notifications;
  state.logs = logs;

  const kindSel = document.getElementById("tr-kind");
  if (kindSel.options.length === 0) {
    kindSel.innerHTML = presets.map((p) => `<option value="${esc(p.kind)}">${esc(p.label)}</option>`).join("");
  }

  renderTrackers();
  renderNotifications();
  renderChannels();
  renderSettings();
  renderLogs();
  // Once per sign-in: loadAll() also runs after tracker/channel edits and must not
  // overwrite an address the admin is typing.
  if (!state.bind) await loadBind();
}

function connectSSE() {
  disconnectSSE();
  const ind = document.getElementById("sse-indicator");
  const label = ind.querySelector(".sse-label");
  const es = new EventSource("/api/events");
  state.eventSource = es;

  es.onopen = async () => {
    ind.dataset.state = "connected";
    label.textContent = "Live";
    // Events emitted before this subscription (initial page load, or while disconnected)
    // are not replayed by the server: reload the snapshot every time the stream opens.
    // The first open follows the bootstrap load closely, but that gap can still drop events.
    try {
      await loadAll();
    } catch {}
  };
  es.onerror = async () => {
    ind.dataset.state = "reconnecting";
    label.textContent = "Reconnecting";
    if (es.readyState !== EventSource.CLOSED || state.eventSource !== es) return;
    // The browser gave up (e.g. the server ended the stream because the session was revoked
    // and the reconnect got 401). Find out whether we are still signed in.
    try {
      const status = await api("/api/auth/status");
      if (!status.authenticated) handleSessionExpired();
      else if (state.eventSource === es) setTimeout(() => state.user && connectSSE(), 5000);
    } catch {
      if (state.eventSource === es) setTimeout(() => state.user && connectSSE(), 5000);
    }
  };
  es.onmessage = (msg) => {
    let ev;
    try {
      ev = JSON.parse(msg.data);
    } catch {
      return;
    }
    if (snapshotLoads > 0) bufferedEvents.push(ev);
    else applyServerEvent(ev, false);
  };
}

function applyServerEvent(ev, replayed) {
  try {
    if (ev.event === "log") {
      // A replayed log line may already be part of the reloaded snapshot.
      if (replayed) {
        const key = JSON.stringify(ev.data);
        if (state.logs.slice(-50).some((l) => JSON.stringify(l) === key)) return;
      }
      state.logs.push(ev.data);
      if (state.logs.length > 300) state.logs.shift();
      renderLogs();
    } else if (ev.event === "tracker_updated") {
      const idx = state.trackers.findIndex((t) => t.id === ev.data.id);
      if (idx >= 0) state.trackers[idx] = ev.data;
      else state.trackers.push(ev.data);
      renderTrackers();
    } else if (ev.event === "new_notification") {
      // Reconcile by ID: a snapshot reload may already contain this entry.
      const existing = state.notifications.findIndex((n) => n.id === ev.data.id);
      if (existing >= 0) state.notifications[existing] = ev.data;
      else state.notifications.unshift(ev.data);
      renderNotifications();
      document.getElementById("stat-notifs").textContent = state.notifications.length;
      updateNavCounts();
    }
  } catch {}
}

async function bootstrap() {
  try {
    const status = await api("/api/auth/status");
    if (status.setup_required) {
      showSetupView(status.setup_token_required);
      return;
    }
    if (!status.authenticated) {
      showLoginView();
      return;
    }
    state.user = status.user;
    state.csrfToken = status.csrf_token;
    showAppShell();
    await loadAll();
    connectSSE();
  } catch (err) {
    showLoginView();
    setAuthError(err.message);
  }
}

bootstrap();
