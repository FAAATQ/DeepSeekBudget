# DeepSeek Budget

[English](README.md) · **简体中文**

> **看见价格，知道什么时候该等。**

<img src="docs/screenshot.zh-CN.png" width="320" alt="弹窗：高峰价，以及距空闲的倒计时">

一个常驻 macOS 菜单栏 / Windows 任务栏的小工具，回答关于 DeepSeek API 价格的两个问题：

1. 现在是**峰价**还是**谷价**？
2. **什么时候变**，还有多久？

图标是 DeepSeek 的鲸鱼，三种颜色 —— **蓝**谷价、**橙**峰价、**灰**未知。图标上不显示数字，
不注册账号，不上报任何数据。它做的事情就一件：把一张时段表变成**扫一眼就能读懂的颜色**。

## 为什么值得常驻

DeepSeek 的 API **谷价是峰价的一半**，而峰谷时段是按 UTC 定义的：

> 峰价为 **01:00–04:00 与 06:00–10:00 UTC，周一至周五**，其余时间均为谷价。

两条推论撑起了这个 app 的大部分价值：

- **北京时间 12:00–14:00 是谷价** —— 两个高峰窗口之间的午休空档。
- **整个周末都是谷价。** 周五 18:00 之后，下一次变价在 **63 小时** 之外。这个倒计时是本工具
  产出的最有用的一个数字，而它恰恰是没人能记在脑子里的东西。

## 安装

从 [**Releases 页面**](https://github.com/FAAATQ/DeepSeekBudget/releases/latest) 下载最新构建。

| 平台 | 产物 | 说明 |
|---|---|---|
| **Windows** | `deepseek-budget.exe` | 免安装绿色版 —— 直接运行，托盘图标就出现了。依赖 WebView2，Windows 11 与较新的 Windows 10 已内置。 |
| **macOS** | `DeepSeek Budget.app` | 拖到任何地方双击即可。构建是 ad-hoc 签名的，首次打开需要 **系统设置 → 隐私与安全性 → 仍要打开**。 |

两个平台都不需要管理员权限，也都不在自己目录之外安装任何东西 —— 这也是它们都能提供
开机自启的原因。

## 它能做什么

- **一个永远正确的托盘图标**，由后台线程上的 Rust 引擎驱动。弹窗关着的时候它照样变色。
- **悬浮提示**：当前状态、下次变价时间、当前价格。Windows 的托盘提示有 127 字符上限，
  所以 Windows 用压缩过的三行形式。
- **弹窗**：今天的完整时段表、倒计时，以及每个模型的价格表。
- **改价不必发新版本。** 设置面板里有一个「检查更新」按钮，从一个小型公开配置仓库拉取最新
  价格与时段 —— 官方改了价，这个 app 不需要发新版本。二进制里内置的那份是兜底，所以
  **点不点这个按钮，app 的行为完全一样**。
- **开机自启**，默认关闭。两个平台都不需要安装器；每次启动都会把登录项重新指向 app 当前
  的位置，所以绿色包被搬走之后仍然有效。
- **中英双语**，跟随系统语言。

### 任何失败都退化成灰色，绝不静默消失

配置损坏、网络响应异常、日期格式写错 —— 每一条失败路径都渲染成灰色的 **未知** 图标外加一句
可读的原因。工作区的 release profile **故意不设** `panic = "abort"`：一个会凭空消失的价格
指示器，比一个老实承认自己不知道的更糟。

远程价格数据被当作**不可信输入**：结构、provider 身份、时段可编译性、日期不可倒退、
价格偏离不超过 20 倍，逐项校验，**任何一项不过就整个拒收** —— 用户当前的数据原样保留，
托盘图标不受影响。

### 网络

这个 app 能发出的对外请求**只有一个**，且只在用户点「检查更新」时发出。不轮询、不启动时
上报、不加遥测。Rust 依赖树里**没有** HTTP 客户端、**没有** TLS 栈 —— 那一次 `fetch` 走的是
webview 自带的 TLS，且受一条只放行**一个域名**的 CSP 约束。

## 开发

前置：Rust（stable ≥ 1.90）、Node（只为拿 Tauri CLI），macOS 上还需要 Xcode Command Line
Tools（**不是**完整 Xcode）。

```bash
npm install                                 # 只为拿 @tauri-apps/cli
npm run dev                                 # 启动，图标出现在菜单栏

cargo test --workspace                      # 127 个测试
cargo test -p deepseekbudget-schedule       # 只跑引擎，约 0.6 秒

npx tauri build --bundles app               # macOS .app
cargo build --release -p deepseek-budget    # Windows 免安装 exe
```

引擎（`crates/schedule`）**零 Tauri 依赖**，且从不自己读系统时钟 —— 「现在」永远是参数传进去的。
这正是每一个峰谷边界都能无头测试的原因。

### 时间旅行

峰谷边界没法靠等来验证 —— 下一个可能在 63 小时之外。所以「现在」可以被钉住：

```bash
DEEPSEEKBUDGET_FAKE_NOW=2026-09-14T02:00:00Z npm run dev   # 周一 10:00 北京 → 峰价
DEEPSEEKBUDGET_FAKE_NOW=2026-09-14T03:59:59Z npm run dev   # 峰价还剩 1 秒
DEEPSEEKBUDGET_FAKE_NOW=2026-09-12T02:00:00Z npm run dev   # 周六 → 整天谷价
```

必须是 RFC 3339。写错了会在 stderr 警告并退回真实时钟，而不是把图标永久钉成灰色。

### 不申请录屏权限也能看 UI

弹窗是离屏渲染的，用的是**真实的** `ui/index.html`、`ui/styles.css` 与 `ui/main.js` ——
生成时读取，从不复制：

```bash
cargo run -q -p deepseekbudget-schedule --example view -- --json 2026-09-14T02:00:00Z 480 CNY zh \
  > /tmp/peak.json
python3 tools/make-preview.py /tmp/peak.json /tmp/peak.html
```

## 已知局限

1. **打开弹窗会抢走焦点**，从你正在用的东西那里。
2. **时区按固定 UTC 偏移建模。** 对没有夏令时的时区是精确的（包括这个 app 主要面向的 UTC+8），
   其他时区则是有据可查的近似。峰谷**判定**永远在 UTC 里做，所以状态永远是对的 ——
   只有显示的本地时间可能差一小时。
3. **「操作系统真的会在登录时执行登录项」是推断，不是观测。** 登录项的内容、自愈、
   以及（Windows 上）「那条命令行确实拉得起托盘图标」都验过，但**两台机器都没有注销或重启过**。
4. **被搬走的绿色包要到*下一次*启动才自愈。** 搬动与那次启动之间的登录就静默地丢了。
5. **macOS 的定位兜底只是兜底。** 拿不到状态项的真实 frame 时，面板锚在菜单栏右端，
   而不是你点的那个图标下面。
6. **没有单实例保护。** 启动两次就是两个托盘图标。
7. **配置错误信息只有英文。**

## 文档

代码可以读。**为什么这么设计、每个平台实际上是什么样**，读不出来。

| 文档 | 内容 |
|---|---|
| [`docs/domain-pricing.md`](docs/domain-pricing.md) | 这个 app 编码的 DeepSeek 峰谷规则、时区是怎么建模的，以及官方 API 究竟能让你计量到什么程度 |
| [`docs/windows.md`](docs/windows.md) | Windows 这一侧付出了什么代价：弹窗定位、前台锁、亚克力为什么必须用直角、127 字符的 tooltip 上限，以及自动化验证 Windows UI 的三个坑 |
| [`docs/macos.md`](docs/macos.md) | 一个返回零高度矩形的 `tray.rect()`、一个收逻辑点却拿到物理像素的显示器查找、圆角归谁裁，以及一条被推翻的「哪个平台更好验证」结论 |
| [`docs/icon-pipeline.md`](docs/icon-pipeline.md) | 托盘图标是怎么产出的、画布为什么是 49×36 而不是正方形，以及与在售菜单栏 app 的实测对比 |

## 许可

MIT，见 [`LICENSE`](LICENSE)。DeepSeek 鲸鱼 logo 归 DeepSeek 所有；本项目与官方无隶属关系。
