# 更新日志

本项目按**功能轮次**推进。V0.1 → V0.3 全部在 **2026-09-12 一天内**完成，随后**无限期搁置封存**；**同日又决定重新发布**，恢复开发。

> 版本号说明：V0.1 / V0.2 / V0.3 是封存期那一天的**内部轮次编号**，它们一起被封为 tag `v0.1.0`。
> 重新开工的成果是**第一次正式发布之后的下一版**，因此记作 **v0.2.0**。

---

## v0.3.2 — 价格自己保持新鲜（2026-09-13）

**问题**：一键同步解决了「改价要发版」，但没解决**没人会去点**。这个 app 常驻菜单栏，大多数人从不打开面板 —— 于是按钮在，价格照旧过期。

**两条路，都不需要你动手**

- **云端**：配置仓库里加了一个 GitHub Action，每天解析官方页面的价格与峰谷时段，有变化就开 PR。**没变化就不开 PR**（`verifiedAt` 只在真有改动时更新），所以不会每天吵你一次
- **本地**：设置里新增「自动检查更新」，默认 **24 小时**，可以选 6/12/24/72 小时或**关闭**。面板上写着你上次自动检查是什么时候

**⚠️ 代价：硬约束 5 第二次放宽**

原来那条是「网络只能有一个口子，**且必须由用户点击触发**」。现在触发者变成了「点击，**或用户设定好的间隔**」—— 这个 app 会在你没操作的时候发一次请求，而它此前明确承诺过不会。缓解措施是默认一天一次、界面上写得清楚、随时能关，**但代价是真的**，完整记录见 [architecture.md §7](docs/design/architecture.md)。请求仍然只是 `GET` 一个公开 JSON，**不上报任何数据、没有遥测**，CSP 仍然只放行一个域名。

**实现上踩到的两个坑**（写下来是因为它们都没报错）

- **窗口只能在主线程建。** 定时器跑在 tick 线程，而 `apply_window_material` 是 AppKit，只在主线程答应。直接调的结果是窗口建出来了、材质被拒，全项目只有一行 stderr 记着：本该 `native glass applied` 的地方印了 `none available` —— **面板的整个观感没了，而没有任何一处报错**
- **`emit` 不是握手。** 建完窗口立刻 `emit("auto-check")`，但建 webview 只是**开始**加载页面，监听器要等 `main.js` 跑到才注册。落在缝里的那次 emit 交给了一个还不存在的页面，而且**静默失败**：「还没有监听器」和「检查过了但无事可做」长得一模一样。现在 Rust 同时置一个旗标，页面启动时用 `take_auto_check` 取走，`swap` 保证不会跑两次

**验证**

本地四条路径，**一个环境变量都不设**，走的是真实分支（不是压过间隔的 `AUTOCHECK_SECONDS`）：

| 场景 | 判据 |
|---|---|
| 设为「关闭」 | 不发请求，**连窗口都不建**（stderr 全空） |
| 默认 24h + 首次运行 | 发请求、`native glass applied`、记下 `lastAutoCheck` |
| 刚检查过（距上次 0 秒） | **不发** —— 不会轮询 |
| 距上次 25 小时 | 发，并刷新时间 |

**它不会打扰你**：窗口是隐藏创建的。跑起来后在过程中连拍三张，弹窗该在的那块区域**像素完全相同**，唯一差异是菜单栏时钟。

**打包后的 `.app` 也单独跑过一遍**（不是只有 debug 构建）：同样三项全过。

**云端**：cron 定时触发实测跑通（把间隔临时压到 5 分钟验的，见 [CLAUDE.md](CLAUDE.md) 的验证纪律），在 GitHub 的网络上跑完整链路并正确判定「无变化、不开 PR」。合并 → purge → CDN 那条链也在 2026-09-14 第一次完整走通。

测试 127 → **133**。

---

## v0.3.1 — 改名，以及两个只有 macOS 才有的原生 bug（2026-09-13）

**改名**

- 产品名 `API Budget` → **`DeepSeek Budget`**。连带改的一共 30 个文件：`productName`、bundle identifier（`com.aicoworks.apibudget` → `com.aicoworks.deepseekbudget`）、Cargo / npm 包名（`api-budget` → `deepseek-budget`、`apibudget-schedule` → `deepseekbudget-schedule`）、二进制名，以及四个开发设施的环境变量前缀（`APIBUDGET_*` → `DEEPSEEKBUDGET_*`）
- ⚠️ **代价**：identifier 一改，**旧的登录项就成了孤儿**（macOS 留下的 `~/Library/LaunchAgents/com.aicoworks.apibudget.plist`）。这正是 [architecture.md §9](docs/design/architecture.md) 早就写下的那条耦合 —— 写的时候是推演，这次真的踩到了

**修复**

- **macOS 四角的「直角框」**。`apply_liquid_glass` 漏传 `content_view`，`window-vibrancy` 的 `move_primary_content_view` 于是在第一行就返回，连带跳过了那个 crate 里**唯一**一次 `masksToBounds` 调用 —— 结果圆角是画出来了，窗口的矩形边界却仍旧画在圆角外面。修法是 `popover.rs::clip_to_card_radius`：给窗口的 view 补上 `cornerRadius` + `masksToBounds`。**只加裁剪，不碰材质**
  - 这个 bug 的隐蔽之处在于**它依赖背景**：面板压在暗色区域上时那条线几乎看不见，压在亮色区域上一眼就能看到。所以「我看不见」和「它好了」不是一回事
- **macOS 面板不锚在菜单栏图标下面**，是两个 bug 叠在一起：
  1. `tray.rect()` 在 macOS 26 上返回 `x=0 y=2100 w=82 h=0` —— 一个**零高度**的框，反推回 AppKit 是 `{{0,0},{41,0}}`。它不是一个位置
  2. `monitor_from_point` 在 macOS 上比对的是 `CGDisplayBounds`，那是**逻辑点**；而托盘 rect 是 `to_physical()` 出来的**物理像素**。2 倍屏上坐标全翻倍，单屏靠运气能中、多屏必选错屏

  两者合起来的效果是：找不到显示器 → 窗口**不被定位** → 菜单栏 app 的面板开在屏幕正中间。修法是退化 rect 守卫 + 单位换算 + `fallback_origin` 兜底到菜单栏右端

**修复：价格同步在目标用户那里是坏的**

- 配置原来从 `raw.githubusercontent.com` 拉，而这个域名**在国内不通**。2026-09-13 实测：**5/5 超时**（每次 19~20 秒），同一份文件的 jsDelivr 镜像 **4/4 通**（约 0.3 秒）。这个 app 的用户主要在国内，等于「检查更新」对他们是废的
- `UPDATE_URL` 与 CSP 的 `connect-src` 一起改成 `cdn.jsdelivr.net` —— **仍然只有一个域名**，硬约束 5 那条「网络只有一个口子」的性质没变
- 代价是缓存，而且是明着付的：jsDelivr 对分支引用发 `s-maxage=43200`（边缘 12 小时）、`max-age=604800`（浏览器 7 天）。浏览器那一半前端早就在用 `cache: "no-store"` 挡掉了；边缘那一半由配置仓库新增的 purge 工作流在每次推送时清掉（已跑通，7 秒）。**试过加查询串击穿 —— 实测无效**，jsDelivr 会把查询串归一化掉，照样返回 `cf-cache-status: HIT`

**修复：`tools/make-preview.py` 从 v0.2.0 起就一直是坏的**

- 它的桩只认四个命令，其余一律 `return null`。价格同步那版加了第五个（`get_provider_status`）之后，预览就不再渲染面板，而是渲染**一条错误横幅** —— 而且什么都不会报错，因为「返回 null 的桩」在有人去读它的字段之前不算错误
- **当初为了抓「Rust 吐出的字段名前端读不到」而造的工具，自己以同样的方式漂移了。** 现在桩从 payload 与 `provider.rs` 推导，未知命令直接抛错（见 retrospective 第 25 条）

**发布打包**

- **版本号统一到 0.3.1**。`tauri.conf.json` 的 `version` 一直是 `0.1.0`，而 release tag 已经是 v0.3.1 —— 用户点「关于」看到的版本号和下载的对不上。工作区 `Cargo.toml` 与 `package.json` 同理（`src-tauri` 和 `crates/schedule` 都是 `version.workspace = true`，改一处就够）
- **macOS 三个产物**（arm64 / x86_64 / universal）换成 0.3.1 并附到 Release
- **Windows exe 重建**：PE 版本资源里 `FileVersion` 与 `ProductVersion` 原本都写着 `0.1.0`。Windows 必须原生构建（Tauri 没有从 macOS 交叉编译到 Windows 的路径），所以在 Windows 机器上重编后重新上传。**下载回来验过**：`FileVersion` / `ProductVersion` 均为 `0.3.1`，且二进制里搜得到 `0.3.1`、搜不到 `0.1.0`

**验证**

- **127 个测试全绿**（90 引擎 + 36 应用 + 1 文档，比上一轮 +3，全部是 `fallback_origin` 的纯函数断言）
- 直角框用**新旧构建同位置相减**验证：差异区域的形状**就是一个直角框**，1.22% 的像素 —— 被移除的正是它
- **macOS 开机自启第一次真机实测**：`launchctl bootstrap` 真的把 app 拉起来了（`state = running`、`runs = 1`）、`reconcile` 从不创建登录项、绿色包搬家后**逐字节自愈**、关闭后清理干净
- **`screencapture` 权限已通** → 「真实菜单栏图标观感」与「液态玻璃观感」这两条从未验过的项目**第一次有了证据**

**沉淀**

- 新增 [docs/platforms/macos.md](docs/platforms/macos.md)，与 windows.md 对称
- [verification.md](docs/practice/verification.md) 新增 4.4 / 4.5 两节：半透明 UI 的边界**必须做对照**、位置**要问不要假设**
- [retrospective.md](docs/practice/retrospective.md) 新增第 21–24 条

**仍未验证**：免安装 exe 在**干净机器**上的 WebView2 依赖；两端开机自启的「**登录时真的会跑**」那一跳

---

## v0.3.0 — 开机自动启动（2026-09-13，未发布）

**新增**
- **开机自动启动**：设置面板里的一个开关（默认关）。Windows 写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，macOS 写 `~/Library/LaunchAgents/<bundle id>.plist`（`RunAtLoad`）—— 两端都**不需要安装器、不需要管理员权限**，这才是一个绿色包能有登录项的原因
- **登录项自愈**：两个平台记的都是**绝对路径**，绿色包一被移动就会静默失效（任务管理器里那条还在，只是下次登录什么都不发生）。所以每次启动跑一次 `reconcile`：**登录项存在 ⇒ 重写成现在这个自己**。实测：把 exe 复制到带空格的目录再运行，注册表里的值自己变成了新路径
- **第四个开发设施 `DEEPSEEKBUDGET_AUTOSTART=on|off`**：不开 GUI 就能写/删登录项，并把它读到的状态打到 stderr（`enabled=Some(false) blocked=true` 这种），于是「被 Windows 挡住」是**问 app 要的**，不是从截图猜的
- 设置面板里的第三种状态：**「关」+「被 Windows 挡住了」** —— 用户在任务管理器里关掉过之后，界面如实这么说，而不是假装开着

**架构变更**
- 新增 `src-tauri/src/autostart.rs`。平台无关的部分是三个**纯函数**（Windows 命令串、LaunchAgent plist、`plan`/`reconcile_action` 的判定），因此能在没有 macOS 的机器上被断言 —— 和 `panel_origin` 同一个套路
- **没有引入官方插件**（`tauri-plugin-autostart` → `auto-launch 0.5.0`）：它写 Windows 注册表时**不给路径加引号**（带参数必失败），macOS 的 `is_enabled()` 只判断 plist **文件在不在**、不比对里面的路径 —— 而后者正是绿色包最需要的那半个语义。依赖增量也更大（会带进第二个 `winreg`）
- 硬约束新增第 9 条：**登录项只能由用户手势创建，`reconcile` 只维护已经存在的那一条**；Windows 的 `StartupApproved\Run` **只读不写**
- 依赖树上**只多一个 `winreg`，而它在锁文件里本来就有**（`embed-resource` 带进来的）。capabilities 与 CSP 都没动 —— [verification.md §6](docs/practice/verification.md) 那条「Rust 侧零网络」的结构性证伪在新代码下依然成立

**验证**：124 个测试全绿（90 引擎 + 33 应用 + 1 文档，比上一轮 +11，全部是纯函数断言）。Windows 上实测：注册表值的内容与引号、被移动后的自愈、被任务管理器关掉时如实报告、以及**把那条命令行原样跑起来确实拉起了托盘图标**（app 自报 `centre=(2234,1416)`，按颜色找图标报 `centre=(2233,1415)` —— 两个独立方法对上）。

其中一条是**验证过程中反过来改掉代码的**：`StartupApproved\Run` 的判定最初照抄 `auto-launch 0.5.0`（看末尾 8 字节是否为零），而本机 `Docker Desktop` 那条真实的禁用记录是 `03 00 00 00` + **全零**时间戳 —— 那个规则会把它读成「开着」。改成看状态字（`02` 开 / `03` 关）后，四个实物形态成了单元测试。

**仍未验证**：**「登录时真的会跑」这一跳** —— 写进去的东西是可执行的，但没注销/重启过；**macOS 侧的全部运行时行为**（本机没有 Mac，那段胶水只做到编译过）。两条都在 [verification.md §7](docs/practice/verification.md) 里。

---

## v0.2.0 — Windows 版 + 价格一键同步（2026-09-12，重新开工）

**新增**
- **Windows 版**：跑通免安装 exe（实测 **6.2 MiB**），亚克力材质实测生效
- **价格一键同步**：官方改价不必再发版。设置面板里点「检查更新」，从独立的配置仓库拉最新的价格与峰谷时段
- **验证设施三件套**：`tools/measure-popover.py`（量布局）、`tools/corner-profile.ps1`（量圆角轮廓）、`tools/diff-region.ps1`（两次截图求差）

**架构变更**
- **「零网络」硬约束被修订为「只有一个口子，且必须由用户点击触发」**。Rust 侧仍然零网络（依赖树无 HTTP 客户端 / TLS 栈），`fetch` 走前端 webview。完整理由与代价见 [architecture.md §7](docs/design/architecture.md)
- 新增 `crates/schedule/src/validate.rs`（远程配置的纯校验）与 `src-tauri/src/provider.rs`（同步副本读写 + `UPDATE_URL`）
- 新增 `crates/schedule/examples/view.rs`：把一份真实 `StateView` 打成 JSON，供验证工具当输入

**修复的缺陷**

| 缺陷 | 症状（实测） | 根因 |
|---|---|---|
| 弹窗跑到屏幕外 | 面板顶端 `y=1446`，在 1440 高的屏幕**之下** | 定位写死「永远在图标下方」；只钳制 X；用整块显示器而非工作区 |
| 弹窗开完就自己关 | `visible=True` → 700ms 后 `False` | Windows 前台锁使窗口从未持有焦点，随后的 `Focused(false)` 关掉了一个没人看见的面板 |
| 点图标关不掉弹窗 | 能打开它的图标关不掉它 | Windows 上焦点丢失**先于**托盘事件到达，顺序与 macOS 相反 |
| **圆角套圆角** | 每个角有两道曲线，缝里露出亚克力 | `apply_acrylic` **不收半径参数**，材质是矩形、圆角归 DWM；CSS 卡片又画了一遍 |
| **设置面板底部够不着** | 卡片 468px，内容 812px，整块价格同步功能不可达 | 固定 320×470 + `resizable(false)` + `overflow: hidden`：溢出**不是「在屏幕外」而是不可达** |

**验证**：113 个测试全绿（90 引擎 + 22 应用 + 1 文档）；`check-update-path.py` 用真实 CSP + 真实 URL + 真实前端跑通整条同步链路；托盘点击链路改由 `tray.rect()` 取坐标后一次到位。

**仍未验证**：真实菜单栏图标观感、液态玻璃观感（同上，见 [verification.md §7](docs/practice/verification.md)）、免安装 exe 在**干净机器**上的 WebView2 依赖。

---

## [封存] 2026-09-12

项目无限期搁置。补齐技术沉淀（`docs/`），不再进行功能开发。

> 此条目记录当时的状态。**同一天晚些时候项目重新开工**，见顶部 v0.2.0。

---

## V0.3 — 官方 logo 托盘图标（2026-09-12）

**变更**
- 托盘图标从「几何圆点」换成 **DeepSeek 官方鲸鱼 logo 剪影**，谷价蓝 `#4d6bfe`、峰价橙 `#ff9500`、Unknown 灰 `#8e8e93`
- 画布从正方形改为跟随 logo 宽高比的 **49×36**，鲸鱼在菜单栏里高 **17.0pt**（正方形画布只能到 12.6pt，**大 1.35 倍**）

**修正**
- **图标导出流水线**：去掉 8× 超采样 + `sips` 缩小，改为**直接渲染到最终尺寸**。墨量 +4%、实心像素 +10%、糊边像素 −20%（[实测表](docs/design/icon-pipeline.md)）
- **画布对齐 bug**：`MARGIN` 原本只在右下生效（实测边距 `L0 R2 T0 B2`），SVG 改为 flex 居中后为 `L1 R1 T1 B1`
- README 里无法复核的「大了 44%」改为可复核的「1.35 倍」
- 清理「绿点」时代的过期表述（README / `tray.rs` 注释 / 脚本 docstring）

**验证**：78 个测试全绿；三个 PNG 字节级确认已编入二进制；`.app` 实测 4.5 MB；启动无解码错误。

---

## V0.2 — 双语 / 货币 / 液态玻璃（2026-09-12）

**新增**
- **中文 / English 切换**，默认跟随系统语言（`sys-locale`，macOS 上零原生依赖）
- **液态玻璃背景**，三级降级：`NSGlassEffectView`（macOS 26+）→ `NSVisualEffectMaterial`（更早）→ 不成则用不透明卡片
- **货币切换**（CNY / USD）—— 原本就有，但藏在设置里没人找到，这轮把它做成价格面板上**可点击的货币控件**

**关键设计**
- **诚实报告玻璃状态**：Rust 记录原生材质是否真的应用成功，通过 `get_environment()` 告诉前端；前端据此设置 `data-glass`。**绝不假装玻璃生效** —— 玻璃没起来时卡片必须保持不透明，否则 10pt 小字会糊在壁纸上看不清
- **i18n 放在引擎里**，沿用 `view.rs` 单一文案源的约束，所以 **tooltip 自动跟随语言**，无需改动
- 删掉 `.card` 的 `backdrop-filter` —— 原生模糊已经做了，CSS 再来一层会双重模糊并互相打架

**体积代价**：`objc2` / `objc2-app-kit` 原本已在依赖树里（tauri 拉的），无重复版本，增量约 **33 KB**。

---

## V0.1 — 初版（2026-09-12）

**核心**：常驻菜单栏的峰谷指示器，回答「现在是什么价」和「什么时候变」。

**技术选型**
- **Tauri v2** 而非 Electron —— 实测产物 **4.5 MB**（Electron 常见 150 MB）。用户的要求：「就这么一个小工具，安装包 150mb 是不是太抽象了点？？我预期就是几 mb」
- **工作区拆分**：`crates/schedule`（零 Tauri 依赖的引擎）+ `src-tauri`（外壳），引擎测试 0.6 秒跑完
- **零网络**：价格数据 `include_str!` 编进二进制；经 `cargo tree` 证实无 HTTP 客户端、无 TLS、tokio 的 `net` feature 未启用

**功能**
- 托盘图标三态（谷价 / 峰价 / Unknown），**图标上不显示价格数字**
- 悬停 tooltip（macOS 5 行 / Windows 压缩形态）
- 点击弹窗（~300px，非 Dashboard）
- **「下次变化」倒计时** —— 本产品最有价值的字段。周五晚间的下一个边界是 **63 小时**之后
- 引擎与价格定义 JSON 完全解耦，加 provider = 加一个 JSON 文件

**测试**：69 个引擎测试，含半开区间边界、63 小时周末间隔、跨整年的一致性不变量、`day_segments` 铺满全天

**修复的 bug**
- `tick.rs` 的 `BOUNDARY_OVERSHOOT` 被 60 秒上限吞掉，常量从未生效（改为先 clamp 再 overshoot）
- `parse_time` 接受 `"01:0"`（chrono 的 `%H:%M` 允许一位数字段），改为严格校验
- 中文区间分隔符用 en dash 而非「至」（**测试是对的，代码是错的**）
- 金额格式：`¥ 8` 多了空格（符号贴紧、字母货币代码加空格）
- 时区标签重复：`now 10:00 · UTC+08:00 (UTC+08:00)`
