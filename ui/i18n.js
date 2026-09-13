// Static UI strings.
//
// Deliberately tiny. Every *dynamic* string — prices, countdowns, tier names, day labels,
// the rule line — is formatted by the Rust engine (crates/schedule/src/view.rs) and arrives
// already translated. Only the chrome around that data lives here, which is why this is a
// couple of dozen entries rather than a translation of the whole app.
//
// A classic script defining one global, not an ES module: this frontend has no bundler, and
// the offscreen preview harness inlines these files into ordinary <script> tags, where
// `export` would be a syntax error.
//
// Entries are either a string, or a function taking the values to interpolate.

window.I18N = (() => {
  const STRINGS = {
    en: {
      prices: "Prices",
      settings: "Settings",
      currency: "Currency",
      timezone: "Timezone",
      language: "Language",
      loading: "Loading…",
      unknown: "Unknown",
      scheduleUnreadable: "The pricing schedule could not be read.",
      unreachable: (error) => `Could not reach the app: ${error}`,
      noUpcomingChange: "no upcoming change",
      clock: (time, zone) => `now ${time} · ${zone}`,
      // e.g. "next Off-Peak in 2h 30m"
      countdown: (state, duration) => `next ${state} in ${duration}`,
      publishedRule: (rule) => `Published rule · ${rule}`,
      switchCurrency: "Click to switch currency",
      switchCurrencyLabel: (code) => `Currency: ${code}. Click to switch.`,
      settingsNote: (zone) => `Currently ${zone}. "System default" follows this computer.`,
      languageNote: "Changing the language also changes the menu bar tooltip.",
      startAtLogin: "Start at login",
      // The chip's visible text *is* the state, so the state is legible without relying on a
      // colour — the same reason the tray icons carry three distinct shapes and not just three
      // colours.
      startAtLoginOn: "On",
      startAtLoginOff: "Off",
      startAtLoginLabel: (state) => `Start at login: ${state}. Click to change.`,
      // The hint says what was written and where, because "start at login" means a different
      // thing on each platform and only one of them is visible in the place people look.
      startAtLoginHintWindows:
        "Added to this user's startup items. Task Manager → Startup lists it, and can turn it off there.",
      startAtLoginHintMac:
        "Written as a LaunchAgent for this user. No installer, no administrator rights.",
      startAtLoginBlocked:
        "Windows is holding this off — switch it back on in Task Manager → Startup.",
      startAtLoginFailed: (error) => `Could not change the login item — ${error}`,
      priceData: "Price data",
      checkUpdate: "Check for updates",
      restoreBuiltIn: "Restore built-in",
      checking: "Checking…",
      upToDate: "Already up to date",
      updatedTo: (date) => `Updated · ${date}`,
      restored: "Restored the built-in data",
      updateFailed: (error) => `Update failed — ${error}`,
      syncHint:
        "Fetches the latest published prices and peak hours, only when you ask. Nothing is sent.",
    },
    zh: {
      prices: "价格",
      settings: "设置",
      currency: "货币",
      timezone: "时区",
      language: "语言",
      loading: "载入中…",
      unknown: "未知",
      scheduleUnreadable: "无法读取价格排程。",
      unreachable: (error) => `无法连接应用：${error}`,
      noUpcomingChange: "无即将到来的变更",
      clock: (time, zone) => `现在 ${time} · ${zone}`,
      countdown: (state, duration) => `距${state}还有 ${duration}`,
      publishedRule: (rule) => `官方规则 · ${rule}`,
      switchCurrency: "点击切换货币",
      switchCurrencyLabel: (code) => `货币：${code}。点击切换。`,
      settingsNote: (zone) => `当前 ${zone}。选择"跟随系统"则使用本机设置。`,
      languageNote: "切换语言也会同时改变菜单栏悬停提示的语言。",
      startAtLogin: "开机自动启动",
      startAtLoginOn: "已开启",
      startAtLoginOff: "已关闭",
      startAtLoginLabel: (state) => `开机自动启动：${state}。点击切换。`,
      startAtLoginHintWindows:
        "写入当前用户的启动项。任务管理器 → 启动 里能看到它，也能在那里关掉。",
      startAtLoginHintMac: "以当前用户的 LaunchAgent 写入，不需要安装器，也不需要管理员权限。",
      startAtLoginBlocked: "被 Windows 挡住了 —— 请在 任务管理器 → 启动 里重新打开。",
      startAtLoginFailed: (error) => `无法修改登录项 —— ${error}`,
      priceData: "价格数据",
      checkUpdate: "检查更新",
      restoreBuiltIn: "恢复内置数据",
      checking: "检查中…",
      upToDate: "已是最新",
      updatedTo: (date) => `已更新 · ${date}`,
      restored: "已恢复内置数据",
      updateFailed: (error) => `更新失败 —— ${error}`,
      syncHint: "获取官方最新价格与峰谷时段，只在你点击时进行。不上报任何数据。",
    },
  };

  let current = "en";

  return {
    /** Switch dictionary. Unknown tags fall back to English, matching the engine's rule. */
    setLocale(tag) {
      current = Object.prototype.hasOwnProperty.call(STRINGS, tag) ? tag : "en";
      // Keep the declared language honest — it drives CJK font selection.
      document.documentElement.lang = current === "zh" ? "zh-Hans" : "en";
    },

    /** Look up a string, interpolating when the entry is a function. */
    t(key, ...args) {
      const entry = STRINGS[current][key] ?? STRINGS.en[key];
      if (entry === undefined) return key;
      return typeof entry === "function" ? entry(...args) : entry;
    },

    /** Fill every element carrying `data-i18n` / `data-i18n-title`. */
    apply(root = document) {
      for (const node of root.querySelectorAll("[data-i18n]")) {
        node.textContent = this.t(node.dataset.i18n);
      }
      for (const node of root.querySelectorAll("[data-i18n-title]")) {
        const text = this.t(node.dataset.i18nTitle);
        node.title = text;
        node.setAttribute("aria-label", text);
      }
    },
  };
})();
