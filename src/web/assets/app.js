"use strict";
// PTNotifier WebUI. Plain browser JavaScript, no build step: the server embeds this file.
// Everything user-controlled is put into the page with textContent/attributes via h(),
// never through innerHTML, so tracker data cannot inject markup.

/* =========================================================================
 * DOM helpers
 * ======================================================================= */
const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

function h(tag, props, ...children) {
  const el = document.createElement(tag);
  if (props) {
    for (const [k, v] of Object.entries(props)) {
      if (v === null || v === undefined || v === false) continue;
      if (k === "class") el.className = v;
      else if (k === "text") el.textContent = v;
      else if (k === "dataset") Object.assign(el.dataset, v);
      else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
      else if (k === "value" || k === "checked") el[k] = v;
      else el.setAttribute(k, v === true ? "" : String(v));
    }
  }
  append(el, children);
  return el;
}

function append(el, children) {
  for (const c of children.flat(Infinity)) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c instanceof Node ? c : String(c));
  }
  return el;
}

const ICONS = {
  radar: "M19.07 4.93A10 10 0 1 0 22 12M16.24 7.76A6 6 0 1 0 18 12M12 12l7-7M12 13a1 1 0 1 0 0-2 1 1 0 0 0 0 2z",
  bell: "M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9M10.3 21a1.94 1.94 0 0 0 3.4 0",
  send: "M22 2 11 13M22 2l-7 20-4-9-9-4z",
  terminal: "M4 17l6-6-6-6M12 19h8",
  sliders: "M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6",
  shield: "M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z",
  plus: "M12 5v14M5 12h14",
  upload: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M17 8l-5-5-5 5M12 3v12",
  play: "M7 4.5v15l12-7.5z",
  pause: "M8 5v14M16 5v14",
  pencil: "M17 3a2.85 2.85 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5z",
  trash: "M3 6h18M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6M10 11v6M14 11v6M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2",
  refresh: "M21 12a9 9 0 1 1-2.64-6.36L21 8M21 3v5h-5",
  external: "M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6",
  rss: "M4 11a9 9 0 0 1 9 9M4 4a16 16 0 0 1 16 16M5 20a1 1 0 1 0 0-2 1 1 0 0 0 0 2z",
  sun: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41",
  moon: "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z",
  monitor: "M3 4h18v12H3zM8 20h8M12 16v4",
  logout: "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4M16 17l5-5-5-5M21 12H9",
  x: "M18 6 6 18M6 6l12 12",
  search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM21 21l-4.3-4.3",
  copy: "M9 9h11v11H9zM5 15H4V4h11v1",
  check: "M20 6 9 17l-5-5",
  alert: "M12 9v4M12 17h.01M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z",
  mail: "M3 5h18v14H3zM3 7l9 6 9-6",
  menu: "M3 6h18M3 12h18M3 18h18",
  zap: "M13 2 3 14h9l-1 8 10-12h-9z",
};

function icon(name) {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("class", "svg-icon");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS(NS, "path");
  path.setAttribute("d", ICONS[name] || "");
  svg.append(path);
  return svg;
}

function hydrateIcons(root = document) {
  for (const el of $$("[data-icon]", root)) {
    if (!el.firstElementChild) el.append(icon(el.dataset.icon));
  }
}

function setIcon(container, name) {
  container.replaceChildren(icon(name));
}

/* =========================================================================
 * Formatting
 * ======================================================================= */
const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" }); // matches the English UI text
const dtf = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const dayFmt = new Intl.DateTimeFormat(undefined, { weekday: "long", day: "numeric", month: "long" });
const timeFmt = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });

function parseDate(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  return isNaN(d) ? null : d;
}

function relTime(iso) {
  const d = parseDate(iso);
  if (!d) return "—";
  const diff = (d.getTime() - Date.now()) / 1000;
  const abs = Math.abs(diff);
  if (abs < 45) return diff < 0 ? "just now" : "in a moment";
  const units = [
    ["year", 31536000],
    ["month", 2592000],
    ["week", 604800],
    ["day", 86400],
    ["hour", 3600],
    ["minute", 60],
  ];
  for (const [unit, secs] of units) {
    if (abs >= secs || unit === "minute") return rtf.format(Math.round(diff / secs), unit);
  }
  return "";
}

function absTime(iso) {
  const d = parseDate(iso);
  return d ? dtf.format(d) : "";
}

/** A <time> that shows relative time and keeps itself fresh (see the ticker in boot()). */
function timeEl(iso, fallback = "—") {
  if (!parseDate(iso)) return h("span", { class: "muted", text: fallback });
  return h("time", { datetime: iso, title: absTime(iso), dataset: { rel: "" }, text: relTime(iso) });
}

function humanDuration(secs) {
  secs = Number(secs);
  if (!isFinite(secs) || secs <= 0) return "";
  if (secs < 60) return `${secs} seconds`;
  const parts = [];
  const d = Math.floor(secs / 86400);
  const hr = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d) parts.push(`${d} day${d > 1 ? "s" : ""}`);
  if (hr) parts.push(`${hr} hour${hr > 1 ? "s" : ""}`);
  if (m) parts.push(`${m} minute${m > 1 ? "s" : ""}`);
  return parts.join(" ") || `${secs} seconds`;
}

function hostOf(url) {
  try {
    return new URL(url).host;
  } catch {
    return url || "";
  }
}

function safeHref(raw) {
  if (!raw) return "";
  try {
    const u = new URL(String(raw).trim());
    if (u.protocol === "http:" || u.protocol === "https:") return u.href;
  } catch {}
  return "";
}

function stripTags(s) {
  if (!s) return "";
  // DOMParser does not run scripts or load resources; we only read its text.
  const doc = new DOMParser().parseFromString(String(s).replace(/<br\s*\/?>/gi, "\n"), "text/html");
  return (doc.body.textContent || "").replace(/\n{3,}/g, "\n\n").trim();
}

/* =========================================================================
 * Per-browser preferences (never required: storage may be unavailable)
 * ======================================================================= */
const prefs = {
  get(key, fallback = null) {
    try {
      const v = localStorage.getItem(`ptn.${key}`);
      return v === null ? fallback : JSON.parse(v);
    } catch {
      return fallback;
    }
  },
  set(key, value) {
    try {
      localStorage.setItem(`ptn.${key}`, JSON.stringify(value));
    } catch {}
  },
};

/* =========================================================================
 * State
 * ======================================================================= */
const state = {
  user: null,
  csrf: null,
  presets: [],
  trackers: [],
  channels: [],
  settings: null,
  bind: null,
  recent: [], // newest notifications, unfiltered (drives the unseen badge)
  feed: [], // notifications page, filtered server-side
  feedLimit: 100,
  logs: [],
  sessions: [],
  rssTokens: [],
  users: [],
  es: null,
};

const ui = {
  view: null,
  trackerFilter: "",
  notifType: "",
  logLevel: "INFO",
  logPaused: false,
  logPending: 0,
  highlightAbove: Infinity,
  importFiles: [],
};

const isAdmin = () => state.user?.role === "admin";

const STATUS = {
  ok: { label: "Healthy", cls: "ok" },
  running: { label: "Checking…", cls: "info running" },
  pending: { label: "Waiting", cls: "" },
  paused: { label: "Paused", cls: "plain" },
  cookie_expired: { label: "Cookies expired", cls: "err" },
  error: { label: "Error", cls: "err" },
};

const CHANNEL_KINDS = {
  discord: { label: "Discord webhook", letter: "D" },
  telegram: { label: "Telegram bot", letter: "T" },
  gotify: { label: "Gotify", letter: "G" },
  ntfy: { label: "ntfy", letter: "n" },
};

const LEVEL_RANK = { DEBUG: 0, TRACE: 0, INFO: 1, WARN: 2, ERROR: 3 };
const MAX_LOGS = 1000;

const needsAttention = (t) => t.last_status === "error" || t.last_status === "cookie_expired";
const presetFor = (kind) => state.presets.find((p) => p.kind === kind);

/* =========================================================================
 * API
 * ======================================================================= */
class ApiError extends Error {
  constructor(message, status) {
    super(message);
    this.status = status;
  }
}

async function api(path, opts = {}) {
  const method = (opts.method || "GET").toUpperCase();
  const headers = { "Content-Type": "application/json", ...(opts.headers || {}) };
  if (method !== "GET" && method !== "HEAD" && state.csrf) headers["X-CSRF-Token"] = state.csrf;
  const body = opts.body !== undefined && typeof opts.body !== "string" ? JSON.stringify(opts.body) : opts.body;

  const res = await fetch(path, { credentials: "same-origin", ...opts, method, headers, body });
  if (res.status === 401 && !path.startsWith("/api/auth/login") && !path.startsWith("/api/auth/setup")) {
    signedOut();
  }
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(data.error || `Request failed (HTTP ${res.status})`, res.status);
  return data;
}

/* =========================================================================
 * Toasts and confirmation
 * ======================================================================= */
function toast(message, kind = "ok") {
  const el = h("div", { class: `toast ${kind}` }, icon(kind === "error" ? "alert" : "check"), h("span", { text: message }));
  $("#toasts").append(el);
  const ttl = kind === "error" ? 6000 : 3200;
  setTimeout(() => {
    el.classList.add("leaving");
    setTimeout(() => el.remove(), 220);
  }, ttl);
}

const fail = (prefix) => (err) => toast(prefix ? `${prefix}: ${err.message}` : err.message, "error");

function confirmAction({ title, text, ok = "Delete", danger = true }) {
  const dlg = $("#confirm-dialog");
  $("#confirm-title").textContent = title;
  $("#confirm-text").textContent = text || "";
  const okBtn = $("#confirm-ok");
  okBtn.textContent = ok;
  okBtn.className = `btn ${danger ? "danger" : "primary"}`;
  dlg.returnValue = "";
  openDialog(dlg);
  $("#confirm-cancel").focus();
  return new Promise((resolve) => {
    dlg.addEventListener("close", () => resolve(dlg.returnValue === "ok"), { once: true });
  });
}

/** Runs an async action with the button shown as busy. */
async function withBusy(btn, fn) {
  if (!btn) return fn();
  btn.classList.add("busy");
  btn.disabled = true;
  try {
    return await fn();
  } finally {
    btn.classList.remove("busy");
    btn.disabled = false;
  }
}

/* =========================================================================
 * Dialogs (native <dialog>: focus trap, Escape and inert background for free)
 * ======================================================================= */
const dialogOpeners = new WeakMap();

function openDialog(dlg) {
  if (dlg.open) return;
  dialogOpeners.set(dlg, document.activeElement);
  dlg.showModal();
}

function closeAllDialogs() {
  for (const d of $$("dialog[open]")) d.close();
}

function initDialogs() {
  for (const dlg of $$("dialog")) {
    let downOnBackdrop = false;
    dlg.addEventListener("mousedown", (e) => (downOnBackdrop = e.target === dlg));
    dlg.addEventListener("click", (e) => {
      if (e.target === dlg && downOnBackdrop) dlg.close();
      if (e.target.closest("[data-close]")) dlg.close();
    });
    dlg.addEventListener("close", () => {
      const opener = dialogOpeners.get(dlg);
      dialogOpeners.delete(dlg);
      if (opener && document.contains(opener) && typeof opener.focus === "function") opener.focus();
    });
  }
}

/* =========================================================================
 * Theme
 * ======================================================================= */
const THEMES = ["system", "light", "dark"];
const THEME_ICON = { system: "monitor", light: "sun", dark: "moon" };

function applyTheme(theme) {
  if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
  const btn = $("#btn-theme");
  btn.title = `Theme: ${theme}`;
  setIcon(btn, THEME_ICON[theme] || "monitor");
}

function cycleTheme() {
  const cur = prefs.get("theme", "system");
  const next = THEMES[(THEMES.indexOf(cur) + 1) % THEMES.length];
  prefs.set("theme", next);
  applyTheme(next);
}

/* =========================================================================
 * Routing
 * ======================================================================= */
const VIEWS = {
  trackers: "Trackers",
  notifications: "Notifications",
  channels: "Channels",
  logs: "Logs",
  settings: "Settings",
  account: "Account & security",
};

function currentRoute() {
  const name = location.hash.replace(/^#\/?/, "").split(/[/?]/)[0];
  return VIEWS[name] ? name : "trackers";
}

function showView(name) {
  const prev = ui.view;
  ui.view = name;
  for (const v of $$(".view")) v.hidden = v.dataset.view !== name;
  for (const a of $$(".nav-link")) {
    if (a.dataset.view === name) a.setAttribute("aria-current", "page");
    else a.removeAttribute("aria-current");
  }
  document.title = `${VIEWS[name]} · PTNotifier`;
  closeMenu();
  if (prev !== name) window.scrollTo(0, 0);

  if (prev === "notifications" && name !== "notifications") ui.highlightAbove = Infinity;
  if (name === "notifications") enterNotifications();
  if (name === "account") loadAccount().catch(fail("Could not load account data"));
  if (name === "logs") scrollLogsToEnd();
}

function closeMenu() {
  $("#sidebar").classList.remove("open");
  $("#scrim").hidden = true;
  $("#btn-menu").setAttribute("aria-expanded", "false");
}

function openMenu() {
  $("#sidebar").classList.add("open");
  $("#scrim").hidden = false;
  $("#btn-menu").setAttribute("aria-expanded", "true");
}

/* =========================================================================
 * Auth screens
 * ======================================================================= */
function showAuth(mode, { tokenRequired = false, error = "" } = {}) {
  $("#shell").hidden = true;
  $("#auth").hidden = false;
  const setup = mode === "setup";
  $("#setup-form").hidden = !setup;
  $("#login-form").hidden = setup;
  $("#auth-sub").textContent = setup ? "First run: create the administrator account" : "Sign in to continue";
  const tokField = $("#setup-token-field");
  tokField.hidden = !tokenRequired;
  $("input", tokField).required = tokenRequired;
  setAuthError(error);
  const first = setup ? $("#setup-form [name=password]") : $("#login-form [name=username]");
  requestAnimationFrame(() => first.focus());
}

function setAuthError(msg) {
  const box = $("#auth-error");
  box.textContent = msg || "";
  box.hidden = !msg;
}

async function enterApp(user, csrf) {
  state.user = user;
  state.csrf = csrf;
  $("#auth").hidden = true;
  $("#shell").hidden = false;
  document.body.classList.toggle("is-admin", isAdmin());
  $("#me-name").textContent = user.username;
  $("#me-role").textContent = isAdmin() ? "Administrator" : "Viewer";
  $("#me-avatar").textContent = (user.username || "?").slice(0, 1);
  $("#settings-fields").disabled = !isAdmin();
  $(".read-only-note").hidden = isAdmin();
  showView(currentRoute());
  await loadAll();
  connectEvents();
}

/** Forget everything that belongs to the account so the next person at this browser sees none of it. */
function signedOut() {
  disconnectEvents();
  state.user = null;
  state.csrf = null;
  Object.assign(state, {
    trackers: [],
    channels: [],
    settings: null,
    bind: null,
    recent: [],
    feed: [],
    logs: [],
    sessions: [],
    rssTokens: [],
    users: [],
  });
  closeAllDialogs();
  for (const f of $$("form")) f.reset();
  ui.importFiles = [];
  $("#rss-reveal").hidden = true;
  $("#rss-url").textContent = "";
  for (const sel of ["#log-box", "#tracker-list", "#channel-grid", "#notif-feed", "#session-list", "#rss-list", "#user-list", "#imp-results", "#bind-status"]) {
    $(sel).replaceChildren();
  }
  document.body.classList.remove("is-admin");
  showAuth("login");
}

function initAuthForms() {
  $("#setup-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const f = e.currentTarget;
    setAuthError("");
    if (f.password.value !== f.confirm.value) return setAuthError("The passwords don’t match.");
    await withBusy(e.submitter, async () => {
      try {
        const res = await api("/api/auth/setup", {
          method: "POST",
          body: { username: f.username.value, password: f.password.value, setup_token: f.setup_token.value || null },
        });
        f.reset();
        await enterApp(res.user, res.csrf_token);
      } catch (err) {
        setAuthError(err.message);
      }
    });
  });

  $("#login-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const f = e.currentTarget;
    setAuthError("");
    await withBusy(e.submitter, async () => {
      try {
        const res = await api("/api/auth/login", {
          method: "POST",
          body: { username: f.username.value, password: f.password.value },
        });
        f.reset();
        await enterApp(res.user, res.csrf_token);
      } catch (err) {
        setAuthError(err.message);
        f.password.select();
      }
    });
  });

  $("#btn-logout").addEventListener("click", async () => {
    try {
      await api("/api/auth/logout", { method: "POST" });
    } catch (err) {
      // A 401 has already signed this tab out. Anything else means the server may still
      // hold the session, so don't pretend the browser is signed out.
      if (state.user) return toast(`Sign out failed: ${err.message}`, "error");
    }
    signedOut();
  });
}

/* =========================================================================
 * Data loading
 * ======================================================================= */
// SSE events that arrive while a snapshot is loading would be overwritten by it,
// so they are buffered and replayed on top of the snapshot.
let snapshotLoads = 0;
let buffered = [];

async function loadAll() {
  snapshotLoads += 1;
  try {
    await loadSnapshot();
  } finally {
    snapshotLoads -= 1;
    if (snapshotLoads === 0 && buffered.length) {
      const pending = buffered;
      buffered = [];
      pending.forEach((ev) => applyEvent(ev, true));
    }
  }
}

async function loadSnapshot() {
  const [presets, ov, logs] = await Promise.all([api("/api/trackers/presets"), api("/api/overview"), api("/api/logs")]);
  state.presets = presets;
  state.settings = ov.settings;
  state.trackers = ov.trackers;
  state.channels = ov.channels;
  state.recent = ov.notifications;
  state.logs = logs.slice(-MAX_LOGS);
  initSeen();

  fillPresetSelects();
  renderTrackers();
  renderChannels();
  renderSettings();
  renderLogs();
  renderTrackerFilterOptions();
  updateUnseen();
  if (ui.view === "notifications") await loadFeed();
  // Once per sign-in: loadAll() also runs after edits and must not wipe an address being typed.
  if (!state.bind && isAdmin()) await loadBind();
}

/* =========================================================================
 * Trackers
 * ======================================================================= */
function engineLabel(kind) {
  return presetFor(kind)?.label || kind;
}

function trackerMatches(t, q) {
  if (ui.trackerFilter === "ok" && !(t.enabled && t.last_status === "ok")) return false;
  if (ui.trackerFilter === "attention" && !needsAttention(t)) return false;
  if (ui.trackerFilter === "paused" && t.enabled) return false;
  if (!q) return true;
  return `${t.name} ${t.kind} ${engineLabel(t.kind)} ${t.base_url}`.toLowerCase().includes(q);
}

function renderTrackers() {
  const all = state.trackers;
  const attention = all.filter(needsAttention).length;
  $("#stat-all").textContent = all.length;
  $("#stat-ok").textContent = all.filter((t) => t.enabled && t.last_status === "ok").length;
  $("#stat-attention").textContent = attention;
  $("#stat-paused").textContent = all.filter((t) => !t.enabled).length;
  $(".stat.err").classList.toggle("zero", attention === 0);
  for (const b of $$("#tracker-stats .stat")) b.setAttribute("aria-pressed", String(b.dataset.filter === ui.trackerFilter));

  const badge = $("#nav-attention");
  badge.hidden = attention === 0;
  badge.textContent = attention;
  badge.title = `${attention} tracker(s) need attention`;

  const list = $("#tracker-list");
  $("#trackers-empty").hidden = all.length > 0;
  $("#tracker-search").closest(".toolbar").hidden = all.length === 0;
  $("#tracker-stats").hidden = all.length === 0;
  if (all.length === 0) return list.replaceChildren();

  const q = $("#tracker-search").value.trim().toLowerCase();
  const rows = all
    .filter((t) => trackerMatches(t, q))
    .sort((a, b) => needsAttention(b) - needsAttention(a) || a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));

  const head = h(
    "div",
    { class: "tracker-head", "aria-hidden": "true" },
    h("span", { text: "Tracker" }),
    h("span", { text: "Status" }),
    h("span", { text: "Last checked" }),
    h("span", { class: "col-next", text: "Next check" }),
    h("span")
  );

  if (rows.length === 0) {
    return list.replaceChildren(
      head,
      h("div", { class: "no-match" }, "No trackers match. ", h("button", { type: "button", class: "link-btn", onclick: clearTrackerFilters, text: "Clear filters" }))
    );
  }
  list.replaceChildren(head, ...rows.map(trackerRow));
}

function clearTrackerFilters() {
  ui.trackerFilter = "";
  $("#tracker-search").value = "";
  renderTrackers();
}

function trackerRow(t) {
  const statusKey = !t.enabled && !needsAttention(t) ? "paused" : t.last_status;
  const st = STATUS[statusKey] || { label: statusKey, cls: "" };
  const admin = isAdmin();
  const actions = admin
    ? h(
        "div",
        { class: "tracker-actions" },
        h("button", { type: "button", class: "icon-btn", title: "Check now", "aria-label": `Check ${t.name} now`, dataset: { act: "run", id: t.id }, disabled: t.last_status === "running" }, icon("refresh")),
        h("button", { type: "button", class: "icon-btn", title: t.enabled ? "Pause" : "Resume", "aria-label": `${t.enabled ? "Pause" : "Resume"} ${t.name}`, dataset: { act: "toggle", id: t.id } }, icon(t.enabled ? "pause" : "play")),
        h("button", { type: "button", class: "icon-btn", title: "Edit", "aria-label": `Edit ${t.name}`, dataset: { act: "edit", id: t.id } }, icon("pencil")),
        h("button", { type: "button", class: "icon-btn danger", title: "Delete", "aria-label": `Delete ${t.name}`, dataset: { act: "delete", id: t.id } }, icon("trash"))
      )
    : h("span");

  const missingAuth = (() => {
    const p = presetFor(t.kind);
    if (!p) return false;
    if (p.auth === "cookies") return !t.cookies_configured;
    if (p.auth === "api_token") return !t.api_token_configured;
    return !t.api_token_configured;
  })();

  return h(
    "div",
    { class: `tracker${t.enabled ? "" : " paused"}`, role: "listitem", dataset: { id: t.id } },
    h(
      "div",
      { class: "tracker-id" },
      h("span", { class: "favicon", "aria-hidden": "true", text: (t.name || "?").trim().slice(0, 1) }),
      h(
        "div",
        { class: "tracker-text" },
        h("div", { class: "tracker-name", title: t.name, text: t.name }),
        h("div", { class: "tracker-sub", title: t.base_url }, hostOf(t.base_url), h("span", { class: "sep", text: "·" }), engineLabel(t.kind))
      )
    ),
    h("div", { class: "col-status" }, h("span", { class: `badge ${st.cls}`, text: st.label })),
    h("div", { class: "when col-last" }, h("span", { class: "cell-label", text: "Last checked" }), timeEl(t.last_checked_at, "Never")),
    h(
      "div",
      { class: `when col-next${t.enabled ? "" : " na"}` },
      h("span", { class: "cell-label", text: "Next" }),
      t.enabled ? timeEl(t.next_check_at, "Not scheduled") : h("span", { class: "muted", text: "Paused" })
    ),
    actions,
    t.last_error && needsAttention(t) ? h("div", { class: "tracker-error", text: t.last_error }) : null,
    !t.last_error && missingAuth ? h("div", { class: "tracker-error warn", text: "No login details saved. Edit the tracker to add them." }) : null
  );
}

async function onTrackerAction(e) {
  const btn = e.target.closest("[data-act]");
  if (!btn) return;
  const t = state.trackers.find((x) => x.id === btn.dataset.id);
  if (!t) return;
  const act = btn.dataset.act;

  if (act === "edit") return openTrackerDialog(t);
  if (act === "run") {
    return withBusy(btn, () =>
      api(`/api/trackers/${encodeURIComponent(t.id)}/run`, { method: "POST" })
        .then(() => toast(`Checking ${t.name}…`))
        .catch(fail("Could not start the check"))
    );
  }
  if (act === "toggle") {
    return withBusy(btn, async () => {
      try {
        const updated = await api(`/api/trackers/${encodeURIComponent(t.id)}`, { method: "PUT", body: { enabled: !t.enabled } });
        upsertTracker(updated);
        toast(updated.enabled ? `${t.name} resumed` : `${t.name} paused`);
      } catch (err) {
        fail("Could not update the tracker")(err);
      }
    });
  }
  if (act === "delete") {
    const ok = await confirmAction({
      title: `Delete ${t.name}?`,
      text: "Its saved cookies and settings are removed. Notifications already captured stay in the history.",
    });
    if (!ok) return;
    try {
      await api(`/api/trackers/${encodeURIComponent(t.id)}`, { method: "DELETE" });
      state.trackers = state.trackers.filter((x) => x.id !== t.id);
      renderTrackers();
      renderTrackerFilterOptions();
      toast(`${t.name} deleted`);
    } catch (err) {
      fail("Could not delete")(err);
    }
  }
}

function upsertTracker(t) {
  if (!t || !t.id) return;
  const i = state.trackers.findIndex((x) => x.id === t.id);
  if (i >= 0) state.trackers[i] = t;
  else state.trackers.push(t);
  renderTrackers();
  renderTrackerFilterOptions();
}

/* ---------- Tracker dialog ---------- */
function fillPresetSelects() {
  const sorted = [...state.presets].sort((a, b) => a.label.localeCompare(b.label));
  const generic = sorted.filter((p) => p.custom_base_url);
  const sites = sorted.filter((p) => !p.custom_base_url);
  const opt = (p) => h("option", { value: p.kind, text: p.label });

  const sel = $("#tracker-form [name=kind]");
  if (!sel.options.length) {
    sel.replaceChildren(h("optgroup", { label: "Generic engines (any site)" }, generic.map(opt)), h("optgroup", { label: "Specific sites" }, sites.map(opt)));
  }
  const imp = $("#imp-kind");
  if (!imp.options.length) {
    const cookie = (p) => p.auth !== "api_token";
    imp.replaceChildren(
      h("option", { value: "", text: "Detect from cookies (known sites only)" }),
      h("optgroup", { label: "Generic engines" }, generic.filter(cookie).map(opt)),
      h("optgroup", { label: "Specific sites" }, sites.filter(cookie).map(opt))
    );
  }
}

function presetDefaultName(p) {
  return p ? p.label.replace(/\s*\(.*\)\s*$/, "") : "";
}

function applyPresetToForm(form, { editing, kindChanged }) {
  const p = presetFor(form.elements.kind.value);
  if (!p) return;
  if (!editing && kindChanged) {
    const prevAuto = form.dataset.autoName || "";
    if (!form.elements.name.value || form.elements.name.value === prevAuto) form.elements.name.value = presetDefaultName(p);
    form.dataset.autoName = presetDefaultName(p);
    form.elements.base_url.value = p.custom_base_url ? "" : p.default_base_url;
  }
  form.elements.base_url.placeholder = p.custom_base_url ? "https://your-tracker.example" : p.default_base_url;

  const notes = $("#tr-notes");
  notes.textContent = p.notes || "";
  notes.hidden = !p.notes;

  const needsCookies = p.auth !== "api_token";
  const needsToken = p.auth !== "cookies";
  $("#tr-cookies-field").hidden = !needsCookies;
  $("#tr-token-field").hidden = !needsToken;
  $("#tr-cookies-label").textContent = p.auth === "cookies_and_api_token" ? "Cookies (optional)" : "Cookies";

  const minSecs = Math.max(60, p.min_interval_secs || 60);
  $("#tr-interval-help").textContent =
    minSecs > 60
      ? `Leave empty to use the global setting. This site is never checked more often than every ${humanDuration(minSecs)}.`
      : "Leave empty to use the global setting.";
}

function openTrackerDialog(t = null) {
  const form = $("#tracker-form");
  form.reset();
  delete form.dataset.autoName;
  const editing = Boolean(t);
  $("#tracker-dialog-title").textContent = editing ? `Edit ${t.name}` : "Add tracker";
  form.elements.id.value = t?.id || "";
  form.elements.kind.disabled = false;

  const cookies = form.elements.cookies_raw;
  const token = form.elements.api_token;
  cookies.placeholder = "Paste a Netscape cookies.txt export, or a header like  uid=123; pass=abc";
  token.placeholder = "";
  $("#tr-clear-cookies").hidden = true;
  $("#tr-clear-token").hidden = true;

  if (editing) {
    form.elements.kind.value = t.kind;
    form.elements.name.value = t.name;
    form.elements.base_url.value = t.base_url;
    form.elements.check_interval_secs.value = t.check_interval_secs ?? "";
    form.elements.mark_as_read.value = t.mark_as_read === null || t.mark_as_read === undefined ? "" : String(t.mark_as_read);
    form.elements.ignore_patterns.value = (t.ignore_patterns || []).join("\n");
    form.elements.enabled.checked = t.enabled;
    if (t.cookies_configured) {
      cookies.placeholder = "Cookies are saved. Paste new ones to replace them.";
      $("#tr-clear-cookies").hidden = false;
    }
    if (t.api_token_configured) {
      token.placeholder = "Saved. Type a new key to replace it.";
      $("#tr-clear-token").hidden = false;
    }
    const adv = t.check_interval_secs || (t.mark_as_read !== null && t.mark_as_read !== undefined) || (t.ignore_patterns || []).length;
    $(".disclosure", form).open = Boolean(adv);
  } else {
    const first = state.presets.find((p) => p.kind === "unit3d") || state.presets[0];
    if (first) form.elements.kind.value = first.kind;
    $(".disclosure", form).open = false;
  }
  applyPresetToForm(form, { editing, kindChanged: !editing });
  openDialog($("#tracker-dialog"));
  (editing ? form.elements.name : form.elements.kind).focus();
}

function initTrackerDialog() {
  const form = $("#tracker-form");
  form.elements.kind.addEventListener("change", () => applyPresetToForm(form, { editing: Boolean(form.elements.id.value), kindChanged: true }));
  $("#tr-cookie-file").addEventListener("change", async (e) => {
    const file = e.target.files?.[0];
    if (file) form.elements.cookies_raw.value = await file.text();
    e.target.value = "";
  });

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const id = form.elements.id.value;
    const interval = form.elements.check_interval_secs.value;
    const mar = form.elements.mark_as_read.value;
    const payload = {
      name: form.elements.name.value.trim(),
      kind: form.elements.kind.value,
      base_url: form.elements.base_url.value.trim(),
      enabled: form.elements.enabled.checked,
      check_interval_secs: interval ? Number(interval) : null,
      mark_as_read: mar === "" ? null : mar === "true",
      ignore_patterns: form.elements.ignore_patterns.value
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean),
    };
    const cookies = form.elements.cookies_raw.value;
    const token = form.elements.api_token.value;
    if (id) {
      payload.clear_cookies = form.elements.clear_cookies.checked;
      payload.clear_api_token = form.elements.clear_api_token.checked;
      if (cookies.trim()) payload.cookies_raw = cookies;
      if (token.trim()) payload.api_token = token;
    } else {
      payload.cookies_raw = cookies;
      payload.api_token = token;
    }

    await withBusy(e.submitter, async () => {
      try {
        const saved = id
          ? await api(`/api/trackers/${encodeURIComponent(id)}`, { method: "PUT", body: payload })
          : await api("/api/trackers", { method: "POST", body: payload });
        $("#tracker-dialog").close();
        upsertTracker(saved);
        toast(id ? "Tracker saved" : `${saved.name || "Tracker"} added`);
      } catch (err) {
        fail("Could not save the tracker")(err);
      }
    });
  });
}

/* ---------- Bulk import ---------- */
const IMPORT_MAX_FILES = 100;
const IMPORT_MAX_FILE_BYTES = 64 * 1024;
const IMPORT_BADGE = { created: "ok", updated: "info", skipped: "plain", error: "err" };

function setImportFiles(files) {
  ui.importFiles = files.filter((f) => /\.txt$/i.test(f.name || f.file?.name || ""));
  const n = ui.importFiles.length;
  const el = $("#imp-selection");
  el.textContent =
    n === 0 ? "No .txt files selected." : n > IMPORT_MAX_FILES ? `${n} files selected. At most ${IMPORT_MAX_FILES} per import.` : `${n} file${n > 1 ? "s" : ""} ready to import.`;
  el.style.color = n > IMPORT_MAX_FILES ? "var(--err)" : "";
}

function openImportDialog() {
  $("#import-form").reset();
  $("#imp-results").replaceChildren();
  setImportFiles([]);
  openDialog($("#import-dialog"));
}

/** Reads dropped files and folders, keeping each file's path inside the folder. */
async function filesFromDrop(dt) {
  const entries = [...dt.items].map((i) => i.webkitGetAsEntry?.()).filter(Boolean);
  if (!entries.length) return [...dt.files].map((f) => ({ file: f, name: f.name }));
  const out = [];
  const walk = async (entry, prefix) => {
    if (out.length > IMPORT_MAX_FILES * 5) return;
    if (entry.isFile) {
      const file = await new Promise((res, rej) => entry.file(res, rej));
      out.push({ file, name: prefix + file.name });
    } else if (entry.isDirectory) {
      const reader = entry.createReader();
      for (;;) {
        const batch = await new Promise((res, rej) => reader.readEntries(res, rej));
        if (!batch.length) break;
        for (const child of batch) await walk(child, `${prefix}${entry.name}/`);
      }
    }
  };
  for (const entry of entries) await walk(entry, "");
  return out;
}

function initImportDialog() {
  const pick = () => {
    const files = [...$("#imp-files").files, ...$("#imp-folder").files].map((f) => ({ file: f, name: f.webkitRelativePath || f.name }));
    setImportFiles(files);
  };
  $("#imp-files").addEventListener("change", pick);
  $("#imp-folder").addEventListener("change", pick);

  const drop = $("#drop");
  drop.addEventListener("dragover", (e) => {
    e.preventDefault();
    drop.classList.add("over");
  });
  drop.addEventListener("dragleave", () => drop.classList.remove("over"));
  drop.addEventListener("drop", async (e) => {
    e.preventDefault();
    drop.classList.remove("over");
    try {
      setImportFiles(await filesFromDrop(e.dataTransfer));
    } catch {
      toast("Could not read the dropped files", "error");
    }
  });

  $("#import-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    // Read the form now: e.currentTarget is null once the handler awaits.
    const kind = $("#imp-kind").value || null;
    const updateExisting = e.currentTarget.elements.update_existing.checked;
    const files = ui.importFiles;
    if (!files.length) return toast("Choose at least one cookies .txt file", "error");
    if (files.length > IMPORT_MAX_FILES) return toast(`At most ${IMPORT_MAX_FILES} files per import`, "error");

    await withBusy($("#imp-submit"), async () => {
      try {
        const tooLarge = [];
        const payloadFiles = [];
        for (const { file, name } of files) {
          if (file.size > IMPORT_MAX_FILE_BYTES) tooLarge.push({ file: name, status: "error", message: "Too large to be a cookie file" });
          else payloadFiles.push({ name, content: await file.text() });
        }
        let results = tooLarge;
        if (payloadFiles.length) {
          const res = await api("/api/trackers/import", {
            method: "POST",
            body: { kind, update_existing: updateExisting, files: payloadFiles },
          });
          results = res.results.concat(tooLarge);
        }
        $("#imp-results").replaceChildren(
          ...results.map((r) =>
            h(
              "div",
              { class: "r" },
              h("div", { class: "r-main" }, h("div", { class: "r-title mono", text: r.file }), h("div", { class: "r-sub", text: r.message })),
              h("span", { class: `badge ${IMPORT_BADGE[r.status] || ""}`, text: r.status })
            )
          )
        );
        const count = (s) => results.filter((r) => r.status === s).length;
        const [created, updated, failed] = [count("created"), count("updated"), count("error")];
        toast(`Imported: ${created} added, ${updated} updated${failed ? `, ${failed} failed` : ""}`, failed && !created && !updated ? "error" : "ok");
        if (created || updated) await loadAll();
      } catch (err) {
        fail("Import failed")(err);
      }
    });
  });
}

/* =========================================================================
 * Notifications
 * ======================================================================= */
function seenKey() {
  return `seen.${state.user?.id || "anon"}`;
}

function initSeen() {
  if (prefs.get(seenKey()) === null) prefs.set(seenKey(), maxId(state.recent));
}

const maxId = (list) => list.reduce((m, n) => Math.max(m, n.id || 0), 0);

function markSeen() {
  const top = Math.max(maxId(state.recent), maxId(state.feed));
  if (top > (prefs.get(seenKey(), 0) || 0)) prefs.set(seenKey(), top);
  updateUnseen();
}

function updateUnseen() {
  const seen = prefs.get(seenKey(), 0) || 0;
  const n = state.recent.filter((x) => x.id > seen).length;
  const badge = $("#nav-unseen");
  badge.hidden = n === 0;
  badge.textContent = n >= 50 ? "50+" : n;
  badge.title = `${n} new`;
}

function renderTrackerFilterOptions() {
  const sel = $("#notif-tracker");
  const cur = sel.value;
  const opts = [...state.trackers].sort((a, b) => a.name.localeCompare(b.name)).map((t) => h("option", { value: t.id, text: t.name }));
  sel.replaceChildren(h("option", { value: "", text: "All trackers" }), ...opts);
  sel.value = state.trackers.some((t) => t.id === cur) ? cur : "";
}

function feedQuery() {
  const params = new URLSearchParams();
  if (ui.notifType) params.set("item_type", ui.notifType);
  const tid = $("#notif-tracker").value;
  if (tid) params.set("tracker_id", tid);
  return params;
}

let feedRequest = 0;

async function loadFeed() {
  const params = feedQuery();
  params.set("limit", String(state.feedLimit));
  // Filters can change while a request is in flight; only the latest one may update the feed.
  const seq = ++feedRequest;
  let feed;
  try {
    feed = await api(`/api/notifications?${params}`);
  } catch (err) {
    if (seq !== feedRequest) return;
    feed = [];
    fail("Could not load notifications")(err);
  }
  if (seq !== feedRequest) return;
  state.feed = feed;
  const rss = feedQuery();
  $("#rss-link").href = rss.toString() ? `/rss?${rss}` : "/rss";
  renderFeed();
  if (ui.view === "notifications" && document.visibilityState === "visible") markSeen();
}

function enterNotifications() {
  ui.highlightAbove = prefs.get(seenKey(), 0) || 0;
  loadFeed();
}

function feedItemMatches(n) {
  if (ui.notifType && n.item_type !== ui.notifType) return false;
  const tid = $("#notif-tracker").value;
  if (tid && n.tracker_id !== tid) return false;
  return true;
}

const TYPE_META = {
  message: { label: "Private message", icon: "mail" },
  error: { label: "Error", icon: "alert" },
  notification: { label: "Notification", icon: "bell" },
};

function dayLabel(d) {
  const today = new Date();
  const start = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((start(today) - start(d)) / 86400000);
  if (diff === 0) return "Today";
  if (diff === 1) return "Yesterday";
  return dayFmt.format(d);
}

function renderFeed() {
  const feed = $("#notif-feed");
  const q = $("#notif-search").value.trim().toLowerCase();
  const time = (n) => parseDate(n.created_at)?.getTime() || 0;
  const items = state.feed
    .filter((n) => !q || [n.tracker_name, n.title, n.subject, n.sender, stripTags(n.body)].join(" ").toLowerCase().includes(q))
    .sort((a, b) => time(b) - time(a) || b.id - a.id);

  if (items.length === 0) {
    const filtered = q || ui.notifType || $("#notif-tracker").value;
    return feed.replaceChildren(
      h(
        "div",
        { class: "card empty" },
        h("span", { class: "empty-icon" }, icon("bell")),
        h("p", { class: "empty-title", text: filtered ? "Nothing matches these filters" : "No notifications yet" }),
        h("p", { class: "empty-text", text: filtered ? "Try a different type, tracker or search." : "Notifications and private messages from your trackers will show up here as they’re found." })
      )
    );
  }

  const groups = new Map();
  for (const n of items) {
    const d = parseDate(n.created_at);
    const key = d ? dayLabel(d) : "Earlier";
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(n);
  }

  const out = [];
  for (const [label, list] of groups) {
    out.push(h("section", { class: "feed-day" }, h("h2", { class: "feed-day-label", text: label }), list.map(feedItem)));
  }
  if (state.feed.length >= state.feedLimit && state.feedLimit < 500) {
    out.push(
      h(
        "div",
        { class: "feed-more" },
        h("button", {
          type: "button",
          class: "btn",
          text: "Load older",
          onclick: (e) =>
            withBusy(e.currentTarget, () => {
              state.feedLimit = Math.min(500, state.feedLimit + 100);
              return loadFeed();
            }),
        })
      )
    );
  }
  feed.replaceChildren(...out);
  revealClippedBodies();
}

/** Shows "Show more" under bodies the 3-line clamp actually cuts off. */
function revealClippedBodies() {
  requestAnimationFrame(() => {
    for (const body of $$("#notif-feed .item-body")) {
      const btn = body.nextElementSibling;
      if (!btn || body.classList.contains("open")) continue;
      btn.hidden = body.scrollHeight <= body.clientHeight + 1;
    }
  });
}

function feedItem(n) {
  const meta = TYPE_META[n.item_type] || TYPE_META.notification;
  const href = safeHref(n.url);
  const body = stripTags(n.body);
  const unseen = n.id > ui.highlightAbove;
  const bodyEl = body ? h("div", { class: "item-body", text: body }) : null;
  // Shown by revealClippedBodies() only when the text is actually clipped at this width.
  const more = body
      ? h("button", {
          type: "button",
          class: "link-btn",
          hidden: true,
          text: "Show more",
          onclick: (e) => {
            const open = bodyEl.classList.toggle("open");
            e.currentTarget.textContent = open ? "Show less" : "Show more";
          },
        })
      : null;

  const when = parseDate(n.created_at);
  return h(
    "article",
    { class: `item ${n.item_type}${unseen ? " unseen" : ""}` },
    h("span", { class: "item-icon", title: meta.label }, icon(meta.icon)),
    h(
      "div",
      { class: "item-main" },
      h(
        "div",
        { class: "item-meta" },
        unseen ? h("span", { class: "new-dot", title: "New" }) : null,
        h("span", { class: "tracker-tag", text: n.tracker_name }),
        h("span", { text: meta.label }),
        n.is_staff ? h("span", { class: "badge accent plain", text: "Staff" }) : null,
        n.sender ? h("span", null, "from ", h("strong", { text: n.sender })) : null,
        when ? h("time", { datetime: n.created_at, title: n.item_date ? `On tracker: ${n.item_date}` : absTime(n.created_at), text: timeFmt.format(when).slice(0, 5) }) : h("span", { text: n.item_date }),
        href ? h("a", { class: "item-link", href, target: "_blank", rel: "noopener noreferrer" }, "Open", icon("external")) : null
      ),
      h("div", { class: "item-title", text: n.subject || n.title || meta.label }),
      bodyEl,
      more
    )
  );
}

function initNotifications() {
  segmented($("#notif-type"), (v) => {
    ui.notifType = v;
    state.feedLimit = 100;
    loadFeed();
  });
  $("#notif-tracker").addEventListener("change", () => {
    state.feedLimit = 100;
    loadFeed();
  });
  $("#notif-search").addEventListener("input", renderFeed);
  let resizeTimer;
  window.addEventListener("resize", () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => ui.view === "notifications" && revealClippedBodies(), 150);
  });
  $("#btn-refresh-notifs").addEventListener("click", (e) => withBusy(e.currentTarget, loadFeed));
}

/* =========================================================================
 * Channels
 * ======================================================================= */
function renderChannels() {
  const grid = $("#channel-grid");
  if (state.channels.length === 0) {
    return grid.replaceChildren(
      h(
        "div",
        { class: "card empty", style: "grid-column:1/-1" },
        h("span", { class: "empty-icon" }, icon("send")),
        h("p", { class: "empty-title", text: "No channels yet" }),
        h("p", { class: "empty-text", text: "Connect Discord, Telegram, Gotify or ntfy to get alerts on your phone or desktop." }),
        isAdmin() ? h("div", { class: "row" }, h("button", { type: "button", class: "btn primary", dataset: { open: "channel" } }, icon("plus"), "Add channel")) : null
      )
    );
  }
  const route = (on, label, ic) => h("span", { class: `route${on ? "" : " off"}`, title: on ? `Sends ${label.toLowerCase()}` : `Doesn’t send ${label.toLowerCase()}` }, icon(ic), label);

  grid.replaceChildren(
    ...state.channels.map((c) => {
      const k = CHANNEL_KINDS[c.kind] || { label: c.kind, letter: "?" };
      return h(
        "article",
        { class: `card channel${c.enabled ? "" : " off"}` },
        h(
          "div",
          { class: "channel-top" },
          h("span", { class: `provider ${c.kind}`, "aria-hidden": "true", text: k.letter }),
          h("div", { style: "min-width:0" }, h("div", { class: "channel-name", text: c.name }), h("div", { class: "channel-kind", text: k.label })),
          h("span", { class: `badge ${c.enabled ? "ok" : "plain"}`, text: c.enabled ? "On" : "Off" })
        ),
        h("div", { class: "routes" }, route(c.notify_notifications, "Notifications", "bell"), route(c.notify_pms, "Messages", "mail"), route(c.notify_errors, "Errors", "alert")),
        isAdmin()
          ? h(
              "div",
              { class: "channel-actions" },
              h("button", { type: "button", class: "btn sm", dataset: { cact: "test", id: c.id } }, icon("zap"), "Send test"),
              h("button", { type: "button", class: "icon-btn", title: "Edit", "aria-label": `Edit ${c.name}`, dataset: { cact: "edit", id: c.id } }, icon("pencil")),
              h("button", { type: "button", class: "icon-btn danger", title: "Delete", "aria-label": `Delete ${c.name}`, dataset: { cact: "delete", id: c.id } }, icon("trash"))
            )
          : null
      );
    })
  );
}

async function onChannelAction(e) {
  const btn = e.target.closest("[data-cact]");
  if (!btn) return;
  const c = state.channels.find((x) => x.id === btn.dataset.id);
  if (!c) return;
  const act = btn.dataset.cact;
  if (act === "edit") return openChannelDialog(c);
  if (act === "test") {
    return withBusy(btn, () =>
      api(`/api/channels/${encodeURIComponent(c.id)}/test`, { method: "POST" })
        .then(() => toast(`Test sent to ${c.name}`))
        .catch(fail("Test failed"))
    );
  }
  if (act === "delete") {
    if (!(await confirmAction({ title: `Delete ${c.name}?`, text: "Alerts will no longer be sent there." }))) return;
    try {
      await api(`/api/channels/${encodeURIComponent(c.id)}`, { method: "DELETE" });
      state.channels = state.channels.filter((x) => x.id !== c.id);
      renderChannels();
      toast(`${c.name} deleted`);
    } catch (err) {
      fail("Could not delete")(err);
    }
  }
}

const CHANNEL_FIELDS = {
  discord: [{ key: "webhook_url", label: "Webhook URL", type: "url", secret: true, full: true, required: true, placeholder: "https://discord.com/api/webhooks/…", help: "Server settings → Integrations → Webhooks." }],
  telegram: [
    { key: "bot_token", label: "Bot token", type: "password", secret: true, full: true, required: true, placeholder: "123456:ABC-DEF…", help: "From @BotFather." },
    { key: "chat_id", label: "Chat ID", required: true, placeholder: "-1001234567890" },
    { key: "topic_id", label: "Topic ID", placeholder: "Optional", help: "For forum groups only." },
  ],
  gotify: [
    { key: "url", label: "Server URL", type: "url", required: true, placeholder: "https://gotify.example.com" },
    { key: "token", label: "App token", type: "password", secret: true, required: true },
  ],
  ntfy: [
    { key: "url", label: "Server URL", type: "url", required: true, default: "https://ntfy.sh" },
    { key: "topic", label: "Topic", required: true, placeholder: "my-private-topic" },
    { key: "token", label: "Access token", type: "password", secret: true, placeholder: "Optional" },
    { key: "priority", label: "Priority", type: "number", min: 1, max: 5, default: 3, help: "1 (lowest) to 5 (urgent)." },
  ],
};

function renderChannelFields(kind, cfg = {}, secrets = []) {
  const wrap = $("#ch-fields");
  wrap.replaceChildren(
    ...(CHANNEL_FIELDS[kind] || []).map((f) => {
      const saved = f.secret && secrets.includes(f.key);
      const val = f.secret ? "" : (cfg[f.key] ?? f.default ?? "");
      return h(
        "label",
        { class: "field", style: f.full ? "grid-column:1/-1" : null },
        h("span", { class: "label", text: f.label }),
        h("input", {
          class: "input mono",
          type: f.type || "text",
          "data-cfg": f.key,
          value: String(val),
          placeholder: saved ? "Saved. Leave empty to keep it." : f.placeholder || "",
          required: f.required && !saved,
          min: f.min,
          max: f.max,
          autocomplete: "off",
          spellcheck: "false",
        }),
        f.help ? h("span", { class: "help", text: f.help }) : null
      );
    })
  );
}

function openChannelDialog(c = null) {
  const form = $("#channel-form");
  form.reset();
  form.elements.id.value = c?.id || "";
  $("#channel-dialog-title").textContent = c ? `Edit ${c.name}` : "Add channel";
  const radios = $$("input[name=kind]", form);
  radios.forEach((r) => (r.disabled = Boolean(c)));
  if (c) {
    form.elements.kind.value = c.kind;
    form.elements.name.value = c.name;
    form.elements.enabled.checked = c.enabled;
    form.elements.notify_notifications.checked = c.notify_notifications;
    form.elements.notify_pms.checked = c.notify_pms;
    form.elements.notify_errors.checked = c.notify_errors;
    renderChannelFields(c.kind, c.config || {}, c.secret_fields || []);
  } else {
    renderChannelFields(form.elements.kind.value);
  }
  openDialog($("#channel-dialog"));
  (c ? form.elements.name : $("input[name=kind]:checked", form)).focus();
}

function initChannelDialog() {
  const form = $("#channel-form");
  $("#ch-kind").addEventListener("change", () => renderChannelFields(form.elements.kind.value));
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const id = form.elements.id.value;
    const kind = form.elements.kind.value;
    const config = {};
    for (const input of $$("[data-cfg]", form)) {
      config[input.dataset.cfg] = input.type === "number" ? Number(input.value) || 0 : input.value.trim();
    }
    if (kind === "ntfy" && !config.priority) config.priority = 3;
    const payload = {
      name: form.elements.name.value.trim(),
      kind,
      enabled: form.elements.enabled.checked,
      config,
      notify_notifications: form.elements.notify_notifications.checked,
      notify_pms: form.elements.notify_pms.checked,
      notify_errors: form.elements.notify_errors.checked,
    };
    await withBusy(e.submitter, async () => {
      try {
        const saved = id
          ? await api(`/api/channels/${encodeURIComponent(id)}`, { method: "PUT", body: payload })
          : await api("/api/channels", { method: "POST", body: payload });
        $("#channel-dialog").close();
        if (saved && saved.id) {
          const i = state.channels.findIndex((x) => x.id === saved.id);
          if (i >= 0) state.channels[i] = saved;
          else state.channels.push(saved);
          renderChannels();
        } else {
          await loadAll();
        }
        toast(id ? "Channel saved" : "Channel added. Send a test to check it works.");
      } catch (err) {
        fail("Could not save the channel")(err);
      }
    });
  });
}

/* =========================================================================
 * Logs
 * ======================================================================= */
function logPasses(l) {
  if ((LEVEL_RANK[l.level] ?? 1) < (LEVEL_RANK[ui.logLevel] ?? 0)) return false;
  const q = $("#log-search").value.trim().toLowerCase();
  return !q || l.message.toLowerCase().includes(q) || (l.target || "").toLowerCase().includes(q);
}

function logLine(l) {
  const d = parseDate(l.timestamp);
  return h(
    "div",
    { class: `log ${l.level}` },
    h("span", { class: "log-ts", title: d ? absTime(l.timestamp) : null, text: d ? timeFmt.format(d) : String(l.timestamp || "").slice(11, 19) }),
    h("span", { class: `log-lvl ${l.level}`, text: l.level }),
    h("span", { class: "log-msg", title: l.target || null, text: l.message })
  );
}

function logsAtBottom() {
  const box = $("#log-box");
  return box.scrollHeight - box.scrollTop - box.clientHeight < 40;
}

function scrollLogsToEnd() {
  const box = $("#log-box");
  requestAnimationFrame(() => (box.scrollTop = box.scrollHeight));
  $("#btn-log-jump").hidden = true;
}

function updateLogCount() {
  const shown = $("#log-box").querySelectorAll(".log").length;
  $("#log-count").textContent = `${shown} of ${state.logs.length} lines`;
}

function renderLogs() {
  const box = $("#log-box");
  const lines = state.logs.filter(logPasses).map(logLine);
  if (lines.length === 0) {
    box.replaceChildren(h("div", { class: "empty" }, h("p", { text: state.logs.length ? "No lines match the current filter." : "Waiting for log output…" })));
  } else {
    box.replaceChildren(...lines);
  }
  updateLogCount();
  scrollLogsToEnd();
}

function pushLog(entry) {
  state.logs.push(entry);
  if (state.logs.length > MAX_LOGS) state.logs.splice(0, state.logs.length - MAX_LOGS);
  if (ui.logPaused) {
    ui.logPending += 1;
    $("#btn-log-follow span:last-child").textContent = `Resume (${ui.logPending} new)`;
    return;
  }
  if (!logPasses(entry)) return updateLogCount();
  const box = $("#log-box");
  const stick = logsAtBottom();
  $(".empty", box)?.remove();
  box.append(logLine(entry));
  while (box.childElementCount > MAX_LOGS) box.firstElementChild.remove();
  if (stick) box.scrollTop = box.scrollHeight;
  else $("#btn-log-jump").hidden = false;
  updateLogCount();
}

function initLogs() {
  segmented($("#log-level"), (v) => {
    ui.logLevel = v;
    prefs.set("logLevel", v);
    renderLogs();
  });
  const savedLevel = prefs.get("logLevel");
  if (savedLevel && LEVEL_RANK[savedLevel] !== undefined) {
    ui.logLevel = savedLevel;
    setSegmented($("#log-level"), savedLevel);
  }
  $("#log-search").addEventListener("input", renderLogs);
  $("#btn-log-clear").addEventListener("click", () => {
    state.logs = [];
    renderLogs();
  });
  $("#btn-log-jump").addEventListener("click", scrollLogsToEnd);
  $("#log-box").addEventListener("scroll", () => {
    if (logsAtBottom()) $("#btn-log-jump").hidden = true;
  });
  $("#btn-log-follow").addEventListener("click", (e) => {
    const btn = e.currentTarget;
    ui.logPaused = !ui.logPaused;
    btn.setAttribute("aria-pressed", String(!ui.logPaused));
    setIcon($("[data-icon]", btn), ui.logPaused ? "play" : "pause");
    $("span:last-child", btn).textContent = ui.logPaused ? "Resume" : "Pause";
    if (!ui.logPaused) {
      ui.logPending = 0;
      renderLogs();
    }
  });
}

/* =========================================================================
 * Settings
 * ======================================================================= */
const SETTING_KEYS = ["default_check_interval_secs", "request_delay_secs", "http_timeout_secs", "discord_embed_limit", "mark_as_read", "send_error_notifications"];

function readSettingsForm() {
  const f = $("#settings-form");
  const out = {};
  for (const k of SETTING_KEYS) {
    const el = f.elements[k];
    out[k] = el.type === "checkbox" ? el.checked : Number(el.value);
  }
  return out;
}

function renderSettings() {
  const s = state.settings;
  if (!s) return;
  const f = $("#settings-form");
  for (const k of SETTING_KEYS) {
    const el = f.elements[k];
    if (el.type === "checkbox") el.checked = Boolean(s[k]);
    else el.value = s[k];
  }
  updateSettingsDirty();
}

function updateSettingsDirty() {
  const s = state.settings;
  if (!s) return;
  const cur = readSettingsForm();
  const dirty = SETTING_KEYS.some((k) => cur[k] !== s[k]);
  $("#settings-dirty").hidden = !dirty;
  $("#btn-settings-save").disabled = !dirty;
  $("#btn-settings-reset").disabled = !dirty;
  const secs = cur.default_check_interval_secs;
  $("[data-humanize=default_check_interval_secs]").textContent = secs >= 60 ? `Every ${humanDuration(secs)}. Minimum 60 seconds.` : "Minimum 60 seconds.";
}

function initSettings() {
  const f = $("#settings-form");
  f.addEventListener("input", updateSettingsDirty);
  f.addEventListener("change", updateSettingsDirty);
  $("#btn-settings-reset").addEventListener("click", renderSettings);
  f.addEventListener("submit", async (e) => {
    e.preventDefault();
    await withBusy($("#btn-settings-save"), async () => {
      try {
        state.settings = await api("/api/settings", { method: "PUT", body: readSettingsForm() });
        renderSettings();
        toast("Settings saved");
      } catch (err) {
        fail("Could not save settings")(err);
      }
    });
    updateSettingsDirty();
  });
}

/* ---------- Network / bind address ---------- */
const BIND_SOURCE = {
  cli: "set by --bind / PTN_BIND",
  database: "saved here",
  default: "default",
  fallback: "fallback, because the saved address couldn’t be used",
};

const isLoopback = (v) => {
  v = (v || "").trim();
  return v === "" || v.startsWith("127.") || v.startsWith("[::1]:") || v.startsWith("localhost:");
};

function updateBindWarning() {
  const v = $("#bind-form").bind.value;
  $("#bind-warning").hidden = !(state.bind && !state.bind.locked && !isLoopback(v));
}

function renderBind() {
  const b = state.bind;
  if (!b) return;
  const kv = [
    ["Listening on", h("span", { class: "v", text: b.active })],
    ["Source", h("span", { text: BIND_SOURCE[b.source] || b.source })],
  ];
  if (b.restart_required) kv.push(["After restart", h("span", null, h("span", { class: "v", text: b.next }), " ", h("span", { class: "badge warn", text: "Restart needed" }))]);
  $("#bind-status").replaceChildren(...kv.flatMap(([k, v]) => [h("span", { class: "k", text: k }), v]));
  $("#bind-locked").hidden = !b.locked;
  $("#bind-edit").hidden = b.locked;
  $("#bind-foot").hidden = b.locked;
  const f = $("#bind-form");
  f.bind.value = b.saved || b.default;
  f.password.value = "";
  updateBindWarning();
}

async function loadBind() {
  try {
    state.bind = await api("/api/server/bind");
    renderBind();
  } catch (err) {
    $("#bind-status").replaceChildren(h("span", { class: "k", text: "Error" }), h("span", { text: err.message }));
  }
}

async function saveBind(bind, btn) {
  const f = $("#bind-form");
  if (!f.password.value) {
    toast("Enter your password to change the address", "error");
    return f.password.focus();
  }
  await withBusy(btn, async () => {
    try {
      state.bind = await api("/api/server/bind", { method: "PUT", body: { bind, password: f.password.value } });
      renderBind();
      toast(state.bind.restart_required ? "Saved. Restart PTNotifier to apply it." : "Saved");
    } catch (err) {
      fail("Could not save")(err);
    }
  });
}

function initBind() {
  const f = $("#bind-form");
  f.bind.addEventListener("input", updateBindWarning);
  for (const b of $$("[data-bind-preset]")) {
    b.addEventListener("click", () => {
      const port = f.bind.value.trim().match(/:(\d+)$/)?.[1] || "8085";
      f.bind.value = `${b.dataset.bindPreset}:${port}`;
      updateBindWarning();
    });
  }
  f.addEventListener("submit", (e) => {
    e.preventDefault();
    saveBind(f.bind.value.trim() || null, e.submitter);
  });
  $("#btn-bind-reset").addEventListener("click", (e) => saveBind(null, e.currentTarget));
}

/* =========================================================================
 * Account & security
 * ======================================================================= */
async function loadAccount() {
  const reqs = [api("/api/auth/rss-tokens"), api("/api/auth/sessions")];
  if (isAdmin()) reqs.push(api("/api/users"));
  const [rss, sessions, users] = await Promise.all(reqs);
  state.rssTokens = rss;
  state.sessions = sessions;
  if (users) state.users = users;
  renderAccount();
}

const emptyRow = (text) => h("div", { class: "empty-row", text });

function describeAgent(ua) {
  if (!ua) return "Unknown device";
  const browser = /Edg\//.test(ua) ? "Edge" : /OPR\//.test(ua) ? "Opera" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : "";
  const os = /Windows/.test(ua) ? "Windows" : /Android/.test(ua) ? "Android" : /iPhone|iPad/.test(ua) ? "iOS" : /Mac OS X/.test(ua) ? "macOS" : /Linux/.test(ua) ? "Linux" : "";
  return browser || os ? [browser, os].filter(Boolean).join(" on ") : ua.slice(0, 60);
}

function renderAccount() {
  $("#session-list").replaceChildren(
    ...(state.sessions.length
      ? state.sessions.map((s) =>
          h(
            "div",
            { class: "r" },
            h(
              "div",
              { class: "r-main" },
              h("div", { class: "r-title" }, describeAgent(s.user_agent), s.current ? h("span", { class: "badge ok plain", text: "This device" }) : null),
              h("div", { class: "r-sub", title: s.user_agent || "" }, h("span", { class: "mono", text: s.ip_address || "unknown address" }), " · active ", timeEl(s.last_seen_at), " · expires ", timeEl(s.expires_at))
            ),
            s.current ? null : h("button", { type: "button", class: "btn sm danger", dataset: { session: s.id_prefix }, text: "Sign out" })
          )
        )
      : [emptyRow("No active sessions.")])
  );
  $("#btn-revoke-others").disabled = !state.sessions.some((s) => !s.current);

  $("#rss-list").replaceChildren(
    ...(state.rssTokens.length
      ? state.rssTokens.map((t) =>
          h(
            "div",
            { class: "r" },
            h(
              "div",
              { class: "r-main" },
              h("div", { class: "r-title" }, t.label || "Unnamed", h("span", { class: "mono muted", text: `${t.token_prefix}…` })),
              h("div", { class: "r-sub" }, "Created ", timeEl(t.created_at), " · last used ", timeEl(t.last_used_at, "never"))
            ),
            h("button", { type: "button", class: "btn sm danger", dataset: { rss: t.id }, text: "Revoke" })
          )
        )
      : [emptyRow("No feed tokens yet.")])
  );

  if (!isAdmin()) return;
  $("#user-list").replaceChildren(
    ...state.users.map((u) => {
      const self = state.user?.id === u.id;
      return h(
        "div",
        { class: "r" },
        h("span", { class: "avatar", "aria-hidden": "true", text: u.username.slice(0, 1) }),
        h(
          "div",
          { class: "r-main" },
          h("div", { class: "r-title" }, u.username, h("span", { class: `badge plain ${u.role === "admin" ? "accent" : ""}`, text: u.role === "admin" ? "Administrator" : "Viewer" }), self ? h("span", { class: "muted", text: "(you)" }) : null),
          h("div", { class: "r-sub" }, "Last sign-in ", timeEl(u.last_login_at, "never"))
        ),
        self ? null : h("button", { type: "button", class: "btn sm danger", dataset: { user: u.id, name: u.username }, text: "Remove" })
      );
    })
  );
}

function initAccount() {
  $("#password-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const f = e.currentTarget;
    if (f.new_password.value !== f.confirm.value) return toast("The new passwords don’t match", "error");
    await withBusy(e.submitter, async () => {
      try {
        await api("/api/auth/password", { method: "POST", body: { current_password: f.current_password.value, new_password: f.new_password.value } });
        f.reset();
        toast("Password changed. Other devices were signed out.");
        await loadAccount();
      } catch (err) {
        fail("Could not change the password")(err);
      }
    });
  });

  $("#rss-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const f = e.currentTarget;
    await withBusy(e.submitter, async () => {
      try {
        const res = await api("/api/auth/rss-tokens", { method: "POST", body: { label: f.label.value } });
        $("#rss-url").textContent = `${location.origin}/rss?token=${encodeURIComponent(res.token)}`;
        $("#rss-reveal").hidden = false;
        f.reset();
        await loadAccount();
      } catch (err) {
        fail("Could not create the token")(err);
      }
    });
  });

  $("#btn-copy-rss").addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText($("#rss-url").textContent);
      toast("Feed address copied");
    } catch {
      toast("Couldn’t copy. Select the address and copy it by hand.", "error");
    }
  });

  $("#btn-revoke-others").addEventListener("click", async (e) => {
    if (!(await confirmAction({ title: "Sign out all other devices?", text: "Every session except this one will have to sign in again.", ok: "Sign them out" }))) return;
    await withBusy(e.currentTarget, () =>
      api("/api/auth/sessions/others", { method: "POST" })
        .then(() => toast("Other devices signed out"))
        .then(loadAccount)
        .catch(fail())
    );
  });

  $("#user-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const f = e.currentTarget;
    await withBusy(e.submitter, async () => {
      try {
        await api("/api/users", { method: "POST", body: { username: f.username.value.trim(), password: f.password.value, role: f.elements.role.value } });
        toast(`${f.username.value.trim()} added`);
        f.reset();
        await loadAccount();
      } catch (err) {
        fail("Could not add the user")(err);
      }
    });
  });

  $("section[data-view=account]").addEventListener("click", async (e) => {
    const s = e.target.closest("[data-session]");
    const r = e.target.closest("[data-rss]");
    const u = e.target.closest("[data-user]");
    try {
      if (s) {
        await withBusy(s, () => api(`/api/auth/sessions/${encodeURIComponent(s.dataset.session)}`, { method: "DELETE" }));
        toast("Device signed out");
      } else if (r) {
        if (!(await confirmAction({ title: "Revoke this feed token?", text: "Readers using it will stop receiving updates.", ok: "Revoke" }))) return;
        await api(`/api/auth/rss-tokens/${encodeURIComponent(r.dataset.rss)}`, { method: "DELETE" });
        toast("Token revoked");
      } else if (u) {
        if (!(await confirmAction({ title: `Remove ${u.dataset.name}?`, text: "They will be signed out and can’t sign in again.", ok: "Remove" }))) return;
        await api(`/api/users/${encodeURIComponent(u.dataset.user)}`, { method: "DELETE" });
        toast(`${u.dataset.name} removed`);
      } else return;
      await loadAccount();
    } catch (err) {
      fail()(err);
    }
  });
}

/* =========================================================================
 * Live events (Server-Sent Events)
 * ======================================================================= */
function setLive(stateName, label) {
  for (const el of [$("#live"), $("#live-mobile")]) el.dataset.state = stateName;
  $("#live .live-label").textContent = label;
  $("#live-mobile").title = label;
}

function disconnectEvents() {
  if (state.es) {
    state.es.close();
    state.es = null;
  }
}

function connectEvents() {
  disconnectEvents();
  const es = new EventSource("/api/events");
  state.es = es;
  setLive("connecting", "Connecting");

  es.onopen = async () => {
    setLive("connected", "Live");
    // The server does not replay events missed while disconnected: reload the snapshot.
    try {
      await loadAll();
    } catch {}
  };
  es.onerror = async () => {
    setLive("reconnecting", "Reconnecting");
    if (es.readyState !== EventSource.CLOSED || state.es !== es) return;
    // The browser gave up (e.g. the session was revoked and the reconnect got 401).
    try {
      const status = await api("/api/auth/status");
      if (!status.authenticated) return signedOut();
    } catch {}
    if (state.es === es) setTimeout(() => state.user && state.es === es && connectEvents(), 5000);
  };
  es.onmessage = (msg) => {
    let ev;
    try {
      ev = JSON.parse(msg.data);
    } catch {
      return;
    }
    if (snapshotLoads > 0) buffered.push(ev);
    else applyEvent(ev, false);
  };
}

function applyEvent(ev, replayed) {
  try {
    if (ev.event === "log") {
      if (replayed) {
        const key = JSON.stringify(ev.data);
        if (state.logs.slice(-50).some((l) => JSON.stringify(l) === key)) return;
      }
      pushLog(ev.data);
    } else if (ev.event === "tracker_updated") {
      upsertTracker(ev.data);
    } else if (ev.event === "new_notification") {
      const n = ev.data;
      const i = state.recent.findIndex((x) => x.id === n.id);
      if (i >= 0) state.recent[i] = n;
      else state.recent.unshift(n);
      state.recent = state.recent.slice(0, 200);
      if (feedItemMatches(n) && !state.feed.some((x) => x.id === n.id)) {
        state.feed.unshift(n);
        if (ui.view === "notifications") renderFeed();
      }
      if (ui.view === "notifications" && document.visibilityState === "visible") markSeen();
      else updateUnseen();
    }
  } catch {}
}

/* =========================================================================
 * Shared widgets
 * ======================================================================= */
function setSegmented(group, value) {
  for (const b of $$("button", group)) b.setAttribute("aria-checked", String(b.dataset.value === value));
}

function segmented(group, onChange) {
  group.addEventListener("click", (e) => {
    const b = e.target.closest("button[data-value]");
    if (!b) return;
    setSegmented(group, b.dataset.value);
    onChange(b.dataset.value);
  });
  group.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    const btns = $$("button", group);
    const i = btns.findIndex((b) => b.getAttribute("aria-checked") === "true");
    const next = btns[(i + (e.key === "ArrowRight" ? 1 : btns.length - 1)) % btns.length];
    next.focus();
    next.click();
  });
}

/* =========================================================================
 * Boot
 * ======================================================================= */
function initShell() {
  $("#btn-theme").addEventListener("click", cycleTheme);
  $("#btn-menu").addEventListener("click", openMenu);
  $("#scrim").addEventListener("click", closeMenu);
  window.addEventListener("hashchange", () => state.user && showView(currentRoute()));

  $("#tracker-stats").addEventListener("click", (e) => {
    const b = e.target.closest(".stat");
    if (!b) return;
    ui.trackerFilter = ui.trackerFilter === b.dataset.filter ? "" : b.dataset.filter;
    renderTrackers();
  });
  $("#tracker-search").addEventListener("input", renderTrackers);
  $("#tracker-list").addEventListener("click", onTrackerAction);
  $("#channel-grid").addEventListener("click", onChannelAction);
  $("#btn-add-tracker").addEventListener("click", () => openTrackerDialog());
  $("#btn-import").addEventListener("click", openImportDialog);

  document.addEventListener("click", (e) => {
    const o = e.target.closest("[data-open]");
    if (!o) return;
    if (o.dataset.open === "tracker") openTrackerDialog();
    else if (o.dataset.open === "import") openImportDialog();
    else if (o.dataset.open === "channel") openChannelDialog();
  });

  // "/" focuses the search box of the current page.
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && $("#sidebar").classList.contains("open")) return closeMenu();
    if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.target.closest("input, textarea, select, [contenteditable]") || $("dialog[open]")) return;
    const input = $(`.view[data-view="${ui.view}"] input[type=search]`);
    if (input) {
      e.preventDefault();
      input.focus();
    }
  });

  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible" && ui.view === "notifications" && state.user) markSeen();
  });

  // Keep relative times ("3 minutes ago") current.
  setInterval(() => {
    for (const el of $$("time[data-rel]")) el.textContent = relTime(el.getAttribute("datetime"));
  }, 30000);
}

async function boot() {
  hydrateIcons();
  applyTheme(prefs.get("theme", "system"));
  initDialogs();
  initShell();
  initAuthForms();
  initTrackerDialog();
  initImportDialog();
  initChannelDialog();
  initNotifications();
  initLogs();
  initSettings();
  initBind();
  initAccount();

  try {
    const status = await api("/api/auth/status");
    if (status.setup_required) return showAuth("setup", { tokenRequired: status.setup_token_required });
    if (!status.authenticated) return showAuth("login");
    await enterApp(status.user, status.csrf_token);
  } catch (err) {
    showAuth("login", { error: err.message });
  }
}

boot();
