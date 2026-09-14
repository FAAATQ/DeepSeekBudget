// Popover renderer.
//
// Deliberately a dumb one: every value it displays — prices, countdowns, tier names, day
// labels, the published rule — is already formatted *and translated* by the Rust engine (see
// crates/schedule/src/view.rs). That is why there is no date maths and no number formatting
// in this file, and why ui/i18n.js only holds the chrome around the data.
//
// The one rule this file must not break: never claim the window has a native glass material
// when it does not. `data-glass` is set from what Rust actually managed to apply.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const t = (key, ...args) => window.I18N.t(key, ...args);

/// Ceiling on a fetched pricing config. The real document is ~2 KB and even a provider with
/// many models would be far under this; the point is that the *size* of an untrusted response
/// is as untrusted as its contents.
const MAX_CONFIG_BYTES = 1024 * 1024;

let settingsOpen = false;
let latestView = null;

/* ---------------------------------------------------------------- helpers */

function tierClass(state) {
  if (state === "peak") return "peak";
  if (state === "off-peak") return "offpeak";
  return "unknown";
}

function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = text;
  return node;
}

/// `Asia/Shanghai (UTC+08:00)` — but the zone name is unavailable on some systems, in which
/// case the app falls back to the offset. Showing "UTC+08:00 (UTC+08:00)" would be silly.
function zoneText(zone, offset) {
  if (!zone) return offset || "";
  if (!offset || zone === offset) return zone;
  return `${zone} (${offset})`;
}

/* --------------------------------------------------------------- rendering */

function render(view) {
  latestView = view;

  // The engine tells us which language it just rendered in; everything else follows from it.
  window.I18N.setLocale(view.locale);
  window.I18N.apply();

  const pill = $("state-pill");
  pill.className = "pill " + tierClass(view.state);
  $("state-label").textContent = view.ok ? view.stateLabel : t("unknown");

  const error = $("error");
  error.hidden = view.ok;
  error.textContent = view.ok ? "" : view.error || t("scheduleUnreadable");

  const countdown = $("countdown");
  clear(countdown);
  if (view.ok && view.nextChange) {
    countdown.textContent = t(
      "countdown",
      view.nextChange.stateLabel,
      view.nextChange.inLabel
    );
  } else if (view.ok) {
    countdown.textContent = t("noUpcomingChange");
  }

  const zone = zoneText(view.displayZoneLabel, view.offsetLabel);
  $("clock").textContent = view.ok ? t("clock", view.nowLabel, zone) : "";

  $("schedule-title").textContent = view.todayLabel || "";
  $("zone-label").textContent = zone;
  renderSegments(view.segments || []);
  $("rule-label").textContent = view.ruleLabel ? t("publishedRule", view.ruleLabel) : "";

  renderCurrencyChip(view);
  renderPrices(view);
  $("tier-note").textContent = view.tierNote;

  // The provenance label is rendered by the engine and comes from the same `StateView` the
  // tooltip is built from, so the footer cannot disagree with the settings panel about which
  // copy of the figures is in force.
  const provenance = view.configSourceLabel
    ? `${view.providerName} · ${view.configSourceLabel}`
    : "";

  $("about-source").textContent = provenance;
  renderNotes(view.notes || []);

  $("foot").textContent = provenance;
}

function renderSegments(segments) {
  const list = $("segments");
  clear(list);
  for (const segment of segments) {
    const item = el("li", segment.isCurrent ? "current" : null);
    item.append(el("span", "swatch " + tierClass(segment.state)));
    item.append(el("span", "range", `${segment.startLabel}–${segment.endLabel}`));
    item.append(el("span", "tier", segment.stateLabel));
    if (segment.isCurrent) item.append(el("span", "marker", "◀"));
    list.append(item);
  }
}

function renderCurrencyChip(view) {
  $("currency-symbol").textContent = view.currencySymbol;
  $("currency-code").textContent = view.currency;
  const button = $("currency-toggle");
  const label = t("switchCurrencyLabel", view.currency);
  button.title = label;
  button.setAttribute("aria-label", label);
  // Nothing to switch to if the provider publishes only one currency.
  button.disabled = (view.availableCurrencies || []).length < 2;
}

function renderPrices(view) {
  const table = $("prices");
  clear(table);

  const models = view.models || [];
  if (!models.length) return;

  const head = el("thead");
  const headRow = el("tr");
  // The units label is translated by the engine, not hardcoded here.
  headRow.append(el("th", null, view.unitsLabel));
  for (const model of models) headRow.append(el("th", null, model.label));
  head.append(headRow);
  table.append(head);

  // Rows line up across models because they come from the same three-tier shape.
  const body = el("tbody");
  const labels = (models[0].rows || []).map((row) => row.label);
  for (let i = 0; i < labels.length; i++) {
    const row = el("tr");
    row.append(el("td", null, labels[i]));
    for (const model of models) {
      const cell = (model.rows || [])[i];
      const td = el("td");
      td.append(el("span", "amount", cell ? cell.active : "—"));
      row.append(td);
    }
    body.append(row);
  }
  table.append(body);
}

function renderNotes(notes) {
  const list = $("notes");
  clear(list);
  for (const note of notes) list.append(el("li", null, note));
}

function renderSettings(data) {
  renderChoiceSelect($("language-select"), data.languageChoices, "tag", data.language);
  renderChoiceSelect($("currency-select"), data.availableCurrencies.map((code) => ({
    value: code,
    label: code,
  })), "value", data.currency);
  renderChoiceSelect(
    $("timezone-select"),
    data.timezoneChoices.map((choice) => ({
      value: choice.minutes,
      label: choice.label,
    })),
    "value",
    data.timezoneOverrideMinutes
  );

  renderAutoCheck(data);

  $("settings-note").textContent = t(
    "settingsNote",
    zoneText(data.zoneLabel, data.offsetLabel)
  );
}

/// How often the app goes and looks for new figures by itself.
///
/// The choices come from Rust, so the list and the default live in one place rather than being
/// restated here and drifting. The note distinguishes "nothing has changed" from "it has not
/// looked" — two facts that would otherwise read identically on a panel whose whole job is to say
/// how current its numbers are.
function renderAutoCheck(data) {
  renderChoiceSelect(
    $("auto-check-select"),
    data.autoCheckChoices.map((choice) => ({
      value: choice.hours,
      label: choice.label,
    })),
    "value",
    data.autoCheckHours
  );

  const note = $("auto-check-note");
  // An ISO date, deliberately: it is a timestamp, and ISO 8601 reads the same in every locale,
  // whereas anything friendlier would have to be rendered by the engine to stay consistent with
  // the rest of the panel.
  note.textContent = data.lastAutoCheck
    ? t("autoCheckLast", String(data.lastAutoCheck).slice(0, 10))
    : data.autoCheckHours === 0
      ? t("autoCheckNote")
      : t("autoCheckNever");
}

/// Which copy of the figures is in force, and the state of the buttons that change it.
///
/// A load-time refusal is shown here rather than in the error banner: the app is running
/// perfectly well on the built-in data, and a red banner would misrepresent that. But staying
/// silent would leave a user whose own file was ignored with no way to find out why.
function renderProviderStatus(status) {
  $("provider-source").textContent = status.sourceLabel || "";
  $("reset-button").hidden = !status.synced;
  $("sync-note").textContent = status.notice ? status.notice : t("syncHint");
  $("sync-note").classList.toggle("bad", Boolean(status.notice));
}

/// Start at login.
///
/// What is shown is what the operating system reports — `set_autostart` writes and then reads
/// back, so a write that failed, or one Windows refuses to honour, shows up here as the state
/// the user will actually get rather than the one they asked for. `enabled: null` means the
/// state could not be read at all, which is not "off" and must not be rendered as such.
function renderAutostart(view) {
  const button = $("autostart-toggle");
  const row = button.parentElement;
  const note = $("autostart-note");

  // A platform with no implementation, or an answer that never arrived: no control is honest,
  // and a control that silently does nothing is worse than no control.
  if (!view || view.supported === false) {
    row.hidden = true;
    note.hidden = true;
    return;
  }
  row.hidden = false;
  note.hidden = false;

  const state =
    view.enabled === true
      ? t("startAtLoginOn")
      : view.enabled === false
        ? t("startAtLoginOff")
        : t("unknown");

  button.textContent = state;
  // Kept for the click handler, which needs to know what it is toggling *from*.
  button.dataset.enabled = view.enabled === true ? "on" : "off";
  const label = t("startAtLoginLabel", state);
  button.title = label;
  button.setAttribute("aria-label", label);

  // Normally the hint explains what was written and where. A refusal or a failure replaces it,
  // because when either happens the mechanism is not what the user needs to know.
  const text = view.blocked
    ? t("startAtLoginBlocked")
    : view.error
      ? t("startAtLoginFailed", view.error)
      : t(platformHintKey());
  note.textContent = text;
  note.classList.toggle("bad", Boolean(view.blocked || view.error));
}

/// Which mechanism the hint should describe. Read from the same `data-platform` the CSS uses,
/// so the sentence and the styling can never disagree about which platform this is.
function platformHintKey() {
  return document.documentElement.dataset.platform === "macos"
    ? "startAtLoginHintMac"
    : "startAtLoginHintWindows";
}

async function refreshAutostart() {
  renderAutostart(await invoke("get_autostart"));
}

/// Transient feedback under the buttons — never the persistent state, which the field above
/// already carries.
function setSyncNote(text, tone) {
  const note = $("sync-note");
  note.textContent = text;
  note.classList.toggle("bad", tone === "bad");
}

/// Both choice lists share the shape: pick the entry whose value matches the current setting,
/// and encode "follow the system" as the empty value.
function renderChoiceSelect(select, choices, valueKey, currentValue) {
  clear(select);
  for (const choice of choices) {
    const raw = valueKey === "tag" ? choice.tag : choice[valueKey];
    const option = el("option", null, choice.label ?? choice.tag ?? choice);
    option.value = raw === null || raw === undefined ? "" : String(raw);
    option.selected =
      (raw === null || raw === undefined ? null : String(raw)) ===
      (currentValue === null || currentValue === undefined ? null : String(currentValue));
    select.append(option);
  }
}

/* ----------------------------------------------------------- price updates */

/// Rust commands reject with the bare string they were handed, and a failed `fetch` rejects with
/// an `Error`. Neither has a dependable `.message`.
function messageOf(error) {
  if (typeof error === "string") return error;
  return error?.message ?? String(error);
}

/// Re-render everything a changed config touches. Prices, the tier note, the footer's
/// provenance line and the list of available currencies all move together — a config swap that
/// refreshed only some of them would leave the panel contradicting itself.
async function afterProviderChange(status) {
  renderProviderStatus(status);
  await refresh();
  await refreshSettings();
}

/// Fetch the published config and hand it to Rust to validate.
///
/// The webview makes the request because it already has a TLS stack, and the Rust side
/// deliberately does not — see the note in `provider.rs` on why the app owns no HTTP client.
/// This function only moves bytes; every decision about whether to *accept* them is Rust's, and
/// is tested there without a webview.
async function checkForUpdates() {
  const button = $("sync-button");
  button.disabled = true;
  setSyncNote(t("checking"));

  try {
    const before = await invoke("get_provider_status");

    // `no-store` earns its place: the host answers with `Cache-Control: max-age=300`, so
    // without it a second click inside five minutes would be served from the webview's own
    // cache and "check for updates" would quietly do nothing.
    const response = await fetch(before.updateUrl, { cache: "no-store" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);

    const body = await response.text();
    // The real document is ~2 KB. A cap costs nothing and means a wrong or hostile response
    // cannot make the webview swallow an arbitrarily large body before Rust ever sees it —
    // this is untrusted input, and the size of it is as untrusted as the contents.
    if (body.length > MAX_CONFIG_BYTES) {
      throw new Error(`${body.length} bytes, over the ${MAX_CONFIG_BYTES} limit`);
    }

    const after = await invoke("apply_provider_config", { json: body });
    await afterProviderChange(after);

    setSyncNote(
      after.verifiedAt === before.verifiedAt
        ? t("upToDate")
        : t("updatedTo", after.verifiedAt)
    );
    return true;
  } catch (error) {
    // Covers every way this fails at once — offline, DNS, a proxy answering with HTML, and
    // every rejection the engine can produce — because from here they are all the same thing:
    // the figures did not change, and here is why.
    setSyncNote(t("updateFailed", messageOf(error)), "bad");
    return false;
  } finally {
    button.disabled = false;
  }
}

async function restoreBuiltIn() {
  const button = $("reset-button");
  button.disabled = true;

  try {
    await afterProviderChange(await invoke("reset_provider_config"));
    setSyncNote(t("restored"));
  } catch (error) {
    setSyncNote(t("updateFailed", messageOf(error)), "bad");
  } finally {
    button.disabled = false;
  }
}

/* ----------------------------------------------------------------- wiring */

async function refresh() {
  render(await invoke("get_view"));
}

async function refreshSettings() {
  renderSettings(await invoke("get_settings"));
}

/// Ask what the window actually got, and say so in the DOM. Defaults to "off" on any error —
/// an opaque panel is always readable, a translucent one over an unblurred wallpaper is not.
///
/// The platform matters for the same reason it is asked for rather than sniffed here: the CSS
/// needs to know whether the OS is already rounding the window. It is not presentational — see
/// the note next to `--card-radius`.
async function applyEnvironment() {
  let glass = false;
  let platform = "";
  try {
    const environment = await invoke("get_environment");
    glass = environment.glass;
    platform = environment.platform ?? "";
  } catch {
    glass = false;
  }
  document.documentElement.dataset.glass = glass ? "on" : "off";
  document.documentElement.dataset.platform = platform;
}

function setSettingsOpen(open) {
  settingsOpen = open;
  $("settings-panel").hidden = !open;
  if (open) $("about-panel").hidden = true;
  $("settings-toggle").classList.toggle("active", open);
}

$("settings-toggle").addEventListener("click", () => setSettingsOpen(!settingsOpen));

$("sync-button").addEventListener("click", checkForUpdates);
$("reset-button").addEventListener("click", restoreBuiltIn);

// Click the currency chip to cycle — the switch sits next to the numbers it changes, which is
// where someone looks for it.
/// Apply a settings change, and put the control back if Rust refuses it.
///
/// Without this a rejection left the panel **lying**: a `<select>` has already committed the
/// user's choice to the DOM by the time the `await` fails, so the control showed the new value
/// while the engine still held the old one — no message, no revert, and nothing on
/// `state-changed` re-renders these, so the mismatch survived until the panel was reopened.
///
/// The revert is by value rather than by re-rendering, because re-rendering is what failed.
async function applySetting(control, command, args, restore) {
  try {
    render(await invoke(command, args));
    // The language row is written by `renderAutostart`, which neither `render` nor
    // `refreshSettings` reaches — so switching language used to leave "On"/"Off" and the
    // platform hint in the language you had just left.
    if (command === "set_language") await refreshAutostart();
  } catch (error) {
    if (control) control.value = restore;
    setSyncNote(t("updateFailed", messageOf(error)), "bad");
  }
  await refreshSettings();
}

$("currency-toggle").addEventListener("click", async () => {
  const currencies = latestView?.availableCurrencies ?? [];
  if (currencies.length < 2) return;
  const next = currencies[(currencies.indexOf(latestView.currency) + 1) % currencies.length];
  await applySetting(null, "set_currency", { currency: next }, null);
});

$("language-select").addEventListener("change", async (event) => {
  const tag = event.target.value;
  await applySetting(event.target, "set_language", { language: tag === "" ? null : tag }, tag);
});

$("currency-select").addEventListener("change", async (event) => {
  const value = event.target.value;
  await applySetting(event.target, "set_currency", { currency: value }, value);
});

$("timezone-select").addEventListener("change", async (event) => {
  const raw = event.target.value;
  await applySetting(
    event.target,
    "set_timezone_offset",
    { minutes: raw === "" ? null : Number(raw) },
    raw
  );
});

$("auto-check-select").addEventListener("change", async (event) => {
  // Rust refuses an interval it does not offer, so a bad value surfaces as a rejection rather
  // than silently becoming a different interval than the one this select is about to show.
  try {
    await invoke("set_auto_check_hours", { hours: Number(event.target.value) });
  } catch (error) {
    setSyncNote(t("updateFailed", messageOf(error)), "bad");
  }
  await refreshSettings();
});

/// The scheduled check, asked for by Rust's tick loop rather than by a click.
///
/// Runs the identical path the button runs, then reports back so Rust can record the attempt and
/// hand the webview back. The report is in a `finally`: **a check that failed still happened**,
/// and retrying it on the next tick would be exactly the polling this app promised not to do.
///
/// `running` guards the two ways this is reached — the boot-time flag and the live event — so a
/// check that is still in flight is never started a second time underneath itself.
let autoCheckRunning = false;

async function runAutoCheck() {
  if (autoCheckRunning) return;
  autoCheckRunning = true;
  let ok = false;
  try {
    ok = await checkForUpdates();
  } finally {
    await invoke("record_auto_check", { ok });
    await refreshSettings();
    autoCheckRunning = false;
  }
}

/// Collect a check the tick loop asked for before this page existed.
///
/// Rust sets a flag when it asks, and emits an event. The event is the path for a panel that is
/// already open; this is the path for the far more common case, where the tick loop created the
/// window and asked in the same breath — before `main.js` had run far enough to be listening.
/// Honour a panel the tray menu asked for before this page existed.
///
/// The popover is created lazily, and the Settings/About menu items *are* a way to create it —
/// so on the first use there is nothing to emit to, and the emit is dropped. Rust records the
/// request as well; this is where it is picked up. One-shot, like `take_auto_check`.
async function collectPanelRequest() {
  const request = await invoke("take_panel_request").catch(() => null);
  if (!request) return;
  if (request === "settings") setSettingsOpen(true);
  if (request === "about") {
    setSettingsOpen(false);
    $("about-panel").hidden = false;
  }
}

/// Collecting on boot is what makes the scheduled check land at all; `take_auto_check` clears the
/// flag, so the two paths together still produce exactly one check.
async function collectAutoCheck() {
  try {
    if (await invoke("take_auto_check")) await runAutoCheck();
  } catch (error) {
    setSyncNote(t("updateFailed", messageOf(error)), "bad");
  }
}

listen("auto-check", collectAutoCheck);

$("autostart-toggle").addEventListener("click", async () => {
  const button = $("autostart-toggle");
  button.disabled = true;
  try {
    // The state to move to comes from what is currently *shown*, which came from the OS — not
    // from a local variable that could have drifted from it.
    const next = button.dataset.enabled !== "on";
    renderAutostart(await invoke("set_autostart", { enabled: next }));
  } catch (error) {
    $("autostart-note").textContent = t("startAtLoginFailed", messageOf(error));
    $("autostart-note").classList.add("bad");
  } finally {
    button.disabled = false;
  }
});

document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  // Escape closes settings first if it is open, so the panel is not dismissed by accident.
  if (settingsOpen) {
    setSettingsOpen(false);
  } else {
    invoke("hide_popover");
  }
});

listen("state-changed", (event) => render(event.payload));

// Both handlers clear the pending record as well as acting on it. Rust sets that record
// whenever the tray menu asks for a panel, because on the first use there is no window to emit
// to and the event is dropped. Clearing it here is what keeps a request handled *now* from
// being replayed on the next launch — a page that is up hears the event, a page that is still
// loading collects the record on boot, and between them exactly one of the two fires.
listen("open-settings", async () => {
  await invoke("take_panel_request").catch(() => {});
  setSettingsOpen(true);
});

listen("open-about", async () => {
  await invoke("take_panel_request").catch(() => {});
  setSettingsOpen(false);
  $("about-panel").hidden = false;
});

(async function init() {
  try {
    // Environment first: the start-at-login hint names a platform-specific mechanism, and it
    // reads the platform off `data-platform`, which this sets.
    await applyEnvironment();
    await refreshSettings();
    await refresh();
    // Again, now that `render()` has told the dictionary which language the engine rendered in.
    // `refreshSettings` writes two notes that carry no `data-i18n` — their text is interpolated
    // — so `I18N.apply()` never revisits them, and a Chinese user read the timezone line and
    // the last-check line in English for the whole session. One extra call at boot.
    await refreshSettings();
    renderProviderStatus(await invoke("get_provider_status"));
    // Not part of `render()`: it is not derived from the price state, and re-reading the
    // registry once a minute to redraw a switch nobody touched would be silly.
    await refreshAutostart();
  } catch (e) {
    $("state-label").textContent = t("unknown");
    const error = $("error");
    error.hidden = false;
    error.textContent = t("unreachable", e);
  }
  // Outside the `try`, and last: this is the only path by which a panel nobody has opened ever
  // fetches anything. A failure while drawing the panel must not also swallow the check.
  await collectAutoCheck();

  // And the same for a panel the tray menu asked for while this page did not yet exist.
  await collectPanelRequest();
})();
