# macOS 这一侧

与 [windows.md](windows.md) 对称的一份。同一个 Tauri 应用在两个平台上不是「同一份代码换个目标」，**坐标系、原生材质的圆角归属、状态栏图标的可问询性**——这几件事两边都不一样，而且差异全藏在默认值和别人的默认值里。

这份文档记录本项目在 macOS 上**真正量到**的东西。

> **一个先放的结论**：本项目原先写着「Windows 比 macOS 更容易验证」，因为 `screencapture` 被屏幕录制权限拒绝。**这条在 2026-09-13 被推翻了**——权限已经通了，macOS 的菜单栏、弹窗、圆角现在都能截图核对。详见本文第五节。

---

## 一、`tray.rect()` 在 macOS 26 上返回的不是一个位置

### 症状（实测）

```
DeepSeek Budget: tray rect x=0 y=2100 w=82 h=0 centre=(41,2100)
```

**两次采样完全一致**，所以它看起来像个可信的值。它不是：`w=82 h=0` 是一个**零高度**的矩形。

反推回 AppKit 就是 `{{0, 0}, {41, 0}}` —— 状态栏按钮的 window 报了一个贴在原点、高度为零的框。

### 根因

`tray-icon` 的 macOS 实现是**现场算**的，不是缓存的：

```rust
// tray-icon-0.24.2/src/platform_impl/macos/mod.rs:515
fn get_tray_rect(window: &NSWindow) -> Rect {
    let frame = window.frame();
    Rect {
        size:     LogicalSize::new(frame.size.width, frame.size.height).to_physical(scale),
        position: LogicalPosition::new(x, flip_window_screen_coordinates(frame.origin.y) - frame.size.height).to_physical(scale),
    }
}
```

`window.frame()` 在这个 macOS 版本上就是退化的。**不是我们的代码算错了，是上游拿不到。**

### 修法

**不要相信它**。`position_under` 现在把「零宽或零高」直接判为「不是位置」：

```rust
let icon_is_usable = icon.width > 0 && icon.height > 0;
```

拿不到就**别装作拿到了**，走 [`fallback_origin`](../../src-tauri/src/popover.rs)：锚到**菜单栏右端**（状态项的所在地），而不是「什么都不做，让窗口停在原地」。

### 代价，说清楚

兜底**只是在猜一个合理位置**，不是找到图标。面板会开在菜单栏右端，而用户点的那个图标可能在左边——**它不会再飘到屏幕中间，但不保证正对图标**。真正的修法是拿到状态栏按钮的真实 frame，那要 `ns_status_item()`，属于上游。

### 为什么以前没被发现

因为失败是**静默**的。原来的代码是：

```rust
let Ok(Some(monitor)) = window.monitor_from_point(centre.0, centre.1) else {
    eprintln!("{APP_NAME}: could not resolve the monitor under the tray icon");
    return;   // ← 窗口从此停在创建时的默认位置
};
```

日志打了一行，窗口**什么都没做**。而「窗口停在创建位置」在屏幕中间——**一个菜单栏 app 的面板开在屏幕正中间，看起来像功能没做完，不像定位失败。**

---

## 二、`monitor_from_point` 收的是逻辑点，不是物理像素

### 症状

多屏时选错屏幕；单屏时**靠运气能中**，所以不会有人发现。

### 根因

托盘 rect 是 `to_physical(scale)` 出来的**物理像素**。而 tao 的 macOS 实现拿它去比对 `CGDisplayBounds`：

```rust
// tao-0.35.3/src/platform_impl/macos/monitor.rs:163
pub fn from_point(x: f64, y: f64) -> Option<MonitorHandle> {
    let bound = CGDisplayBounds(monitor.0);     // ← 逻辑点
    if CGRectContainsPoint(bound, CGPoint::new(x, y)) > 0 { return Some(monitor); }
}
```

**2 倍屏上每个坐标都翻倍。**

### 实测量到的后果

退化 rect 的中心 `(41, 2100)` 当逻辑点看，落在一个 `1680×1050` 的桌面之外——于是 `y=2100` 越界，返回 `None`，面板永远不被定位。**两个 bug 叠在一起才产生了「面板在屏幕中间」这一个现象。**

### 修法

问之前先换算：

```rust
let centre = (
    (icon.x as f64 + icon.width as f64 / 2.0) / scale,
    (icon.y as f64 + icon.height as f64 / 2.0) / scale,
);
```

> 这条的教训不是「记得除以 scale」，而是 **`monitor_from_point` 的单位在平台之间不一致**。窗口、显示器、托盘三套 API 混在一起时，单位要靠**读上游实现**确认，不能靠文档——那句文档只有一行「Returns the monitor that contains the given point」。

---

## 三、圆角：谁裁这一刀

### 症状

面板**四个角的外面**有一圈细线的直角框。直边处被圆角面板盖住，只有四个角露头——所以看起来像「四个尖尖」。

### 根因

窗口是**矩形**。圆角由 `window-vibrancy` 挂上的玻璃视图画（`setCornerRadius(CARD_RADIUS)`），**但没有任何一层设过 `masksToBounds`**。

`window-vibrancy` 本来会做，只是仅限我们不走的路径：

```rust
fn move_primary_content_view(container, content_view, glass, radius) {
    let Some(content) = content_view else {
        return;                       // ← 没传 content_view，这里直接返回
    };
    if radius > 0.0 {
        unsafe { apply_corner_radius_layer(target_view, radius) };  // ← 唯一的 masksToBounds
    }
```

我们的调用是 `LiquidGlassOptions::new(style).radius(CARD_RADIUS)` —— **没传 `content_view`**，于是这个函数当场返回，webview 既没被搬进玻璃视图，也没拿到圆角裁剪。

### 修法

```rust
// popover.rs::clip_to_card_radius
let layer = msg_send![view, layer];
msg_send![layer, setCornerRadius: CARD_RADIUS];
msg_send![layer, setMasksToBounds: Bool::YES];
```

裁在 `ns_view()` 上，玻璃和 webview 一起被裁。**只加裁剪，不碰材质**——玻璃的模糊、色调一个字没改，因为它本来就该被裁成圆的。

代价：`objc2` 成为一个直接依赖。版本与 tauri 带进来的一致（0.6.4），**锁文件零新增**。

### 怎么确认它真的修好了：新旧构建同位置相减

这个 bug 害人之处在于**它依赖背景**——面板压在暗色区域上时那条线几乎看不见，压在亮色区域上时一眼就能看到。所以「我看不见」和「它好了」不是一回事。

做法（两个构建自报同一个位置，所以是同位置同背景的严格对照）：

```bash
# 旧构建先拍一张，新构建再拍一张，然后逐像素相减
python3 - <<'PY'
# 差异图本身就是一个直角 —— 那就是被移除的东西
PY
```

**验证的结果：差异区域的形状就是一个直角框**，1.22% 的像素，竖边、横边、交于窗口矩形角；修复后只剩圆角边缘的亚像素位移。**用相减代替眼睛**，和 Windows 那边用 `corner-profile.ps1` 代替眼睛是同一个方法。

---

## 四、开机自启：真机实测（2026-09-13）

此前这一栏写的是「**全部没验过**」。现在验过的部分：

| 环节 | 证据 | 结果 |
|---|---|---|
| plist 落盘 | `~/Library/LaunchAgents/com.aicoworks.deepseekbudget.plist`，474 字节 | ✅ |
| 内容 | `ProgramArguments` 指向**包内**可执行文件（`Contents/MacOS/…`），`RunAtLoad` = `true` | ✅ |
| 路径带空格**不加引号** | plist 数组不是 shell 字符串，加了反而找不到文件 | ✅ |
| `plutil -lint` | `OK` | ✅ |
| app 回读状态 | `enabled=Some(true) blocked=false error=None`（从系统**回读**，不是回声） | ✅ |
| **launchctl 加载** | `bootstrap` 退出码 0；`state = running`、`runs = 1`、`pid` 非空 | ✅ |
| **reconcile 不创建** | 没有登录项时跑 app，plist 依然不存在 | ✅ |
| **reconcile 自愈** | 路径改到 `/Volumes/OldUSB/…`，跑一次 app 后**逐字节还原** | ✅ |
| 关闭清理 | plist 删除、无 `.plist.writing` 残留、launchd 无登记、无残留进程 | ✅ |

### 仍然没验的那一跳

**「注销/重启后 macOS 是否真的会去加载它」**。验到的是「launchd 收到加载命令时能跑通」——`launchctl bootstrap` 就是**登录时 launchd 做的同一件事**，所以性质从「我们的代码从没跑过」变成了「只剩 OS 的调度时机」。

另一个没查的：macOS 26 对「后台项目」管得很严，那个 plist 会不会出现在 *系统设置 → 通用 → 登录项*、会不会弹通知。`sfltool dumpbtm` 需要 root，无法非交互查询。

---

## 五、可验证性：这条结论被推翻了

[windows.md](windows.md) 开头写着「**Windows 比 macOS 更容易验证**」，理由是 `screencapture` 被屏幕录制权限拒绝。

**2026-09-13 起不成立**：权限已通，`screencapture -x -R...` 正常出图。于是：

- **真实菜单栏里的图标观感** —— 拍到了，蓝色鲸鱼，`49×36` 的 PNG 在 Retina 上按 @2x 算是 `24.5×18 pt`，与 [icon-pipeline.md](icon-pipeline.md) 记的「16–18pt 高」标准吻合
- **液态玻璃的实际观感** —— `DEEPSEEKBUDGET_OPEN_POPOVER=1` 打开弹窗即可截图

**教训**：平台的「可验证性」是一个会变的量。它写进文档时是真的，但**权限、工具、显示器这些前提一变，结论就作废**——所以这类判断必须带上测量日期和前提，否则会被后人当成事实继续引用。

---

## 六、`fallback_origin` 与「测量工具会骗人」

一个本条新增的坑，和「测量工具会骗人」是同一类：

**`DEEPSEEKBUDGET_REPORT_TRAY_RECT` 报的弹窗坐标是过期的。**

`position_under` 调 `set_position` 之后，`report_rect_if_asked` 立刻读 `outer_position()` —— 但 `set_position` 是**异步投递**给窗口服务器的，此时还没生效。于是这个开发设施会**报出一个自信但过期**的坐标。

实测：日志说 `popover rect x=1360 y=306`（旧位置），而截图显示面板已经在右上角。

**它不是坏了，它是提前了。** 判定位置要看**截图**，不要看这一行。
