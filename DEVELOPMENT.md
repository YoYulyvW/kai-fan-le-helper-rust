# 开饭了助手 Rust 重构开发记录

## 项目

- 项目名称：开饭了助手
- Rust 项目：kai-fan-le-helper
- 原项目：Python + PySide2
- Rust 项目路径：D:\编程\zhushou\kai-fan-le-helper-rust
- 原项目路径：D:\编程\zhushou\kai-fan-le-helper

## 工具链

- Rust 1.77.2（最后一个官方支持 Windows 7 的版本），由 rust-toolchain.toml 固定。
- 本机采用项目内便携安装：工具链位于 .cargo / .rustup（已加入 .gitignore）。
- 编译依赖 MSVC 链接器；本地未安装 Build Tools，改用 GitHub Actions（windows runner 自带 MSVC）编译。

## 重构目标

在不改变原 Python 版本外部行为的前提下，将项目逐步重构为 Rust Windows 桌面应用。

核心原则：

1. 网络协议、端口、消息格式保持兼容。
2. 原有功能不得无故删除。
3. 配置文件尽量保持向后兼容。
4. Windows 7/10/11 支持。
5. 支持系统托盘。
6. UI 与网络、热键等后台任务解耦。
7. 尽量减少 unsafe，Windows API 集中封装。
8. UI 保持轻量、小巧、无边框、始终置顶的工具条形态。
9. 不为了视觉效果增加不必要的动画和复杂布局。
10. 每个阶段必须能够独立编译和检查。

## 协议对照（必须保持不变）

| 项目 | 值 |
|------|----|
| 通信端口 TCP | 8848 |
| UDP 广播端口 | 8849 |
| TCP 握手端口 | 8850 |
| UDP 广播消息 | {"magic":"KFL","action":"hello","port":8848,...} |
| 探测请求 | GET /ping HTTP/1.0 |
| 发送请求 | POST /submit，body {"text":"..."} |
| User-Agent | KaiFanLe-Helper/1.0 |
| 默认热键 | F1 |
| 自启注册表 | HKCU\Software\Microsoft\Windows\CurrentVersion\Run\KaiFanLeHelper |

## 配置文件

- 设置：~/.kai_fan_le_helper_settings.json
- 历史：~/.kai_fan_le_helper_history.json
- 映射：mappings.txt（优先 exe 同目录，其次 ~/.mappings.txt）

## 开发阶段

### 阶段 1：原项目分析 — 已完成

### 阶段 2：Rust 项目骨架 — 已完成

- Cargo.toml、rust-toolchain.toml
- GitHub Actions 构建管线
- 模块骨架：app / config / core / network / platform / ui

### 阶段 3：配置、网络、Windows 基础能力 — 进行中

- config.rs：设置/历史/映射读写与兼容（完成）
- core/title.rs：剧名解析（完成，含测试）
- core/mod.rs：剪贴板过滤、姓名生成（完成，含测试）
- core/net.rs：协议消息与解析（完成，含测试）
- network/mod.rs：UDP 发现、TCP 握手、HTTP 发送、网段扫描（完成）
- platform/autostart.rs：开机自启（完成）
- platform/input.rs / hotkey.rs：接口定义，待实现

### 阶段 4：核心业务 — 待开始

### 阶段 5：全局热键与 SendInput — 待开始

### 阶段 6：UI 与系统托盘 — 待开始

### 阶段 6：UI 与系统托盘 — 基本完成

已实现：

- Win32 无边框置顶工具窗口（可拖动）
- 工具条控件：状态栏、输入框、历史/名字/发送/关闭按钮
- 原生 Shell_NotifyIcon 托盘 + 右键菜单（显示/隐藏、重新扫描、设置、退出）
- 设备/历史/映射三类弹窗（列表框 + 双击选择）
- 完整闭环：热键→生成名字→判断前台→Ctrl+V 粘贴 / 填入输入框
- 剪贴板监听（500ms 轮询 + 去重）+ 分享文本解析
- 心跳探测离线设备
- 发送按钮→POST /submit

剩余：设置页（热键/缩放/主题可视化选择）、热键看门狗、图标资源。

### 阶段 7：性能、打包与最终验收 — 基本完成

- 打包说明：见 PACKAGING.md
- 验收报告：见 ACCEPTANCE.md
- 实测：Release 二进制约 95–175 KB，16 个单元测试全绿，CI 全绿。

实测数据（CI，run #6）：

- 工具链：Rust 1.77.2 x86_64-pc-windows-msvc
- Release 二进制体积：约 95–175 KB（远低于 15 MB 目标）
- 单元测试：16 个全部通过
- CI：GitHub Actions windows-latest，构建 + 测试 + 产物上传全绿

## 当前实现状态（诚实记录）

已实现且可编译、CI 通过：

- 工具链：项目内 Rust 1.77.2（Win7 兼容），GitHub Actions 编译。
- config：设置/历史/映射读写，字段带默认值，向后兼容。
- core：剧名解析、剪贴板过滤、随机姓名、设备会话、协议消息（含 16 个单元测试）。
- network：UDP 广播监听、TCP 握手监听、HTTP 发送、/24 网段并发扫描。
- platform：WH_KEYBOARD_LL 全局热键钩子（独立线程 + 消息循环 + 300ms 节流）、
  SendInput 扫描码 Ctrl+V、注册表开机自启、系统主题检测。
- app：事件通道汇聚网络/热键事件并驱动业务。
- ui：Win32 无边框置顶工具窗口 + 原生 Shell_NotifyIcon 托盘（含右键菜单）。

尚未完成（阶段 6 剩余 / 阶段 7）：

- 工具条控件：输入框、发送/历史/名字/关闭按钮、状态双行显示。
- 剪贴板监听与防抖、剪贴板实体化。
- 快捷映射弹窗、历史弹窗、设备选择弹窗。
- 热键→生成名字→判断前台→Ctrl+V 粘贴的完整闭环。
- 设置页（热键选择、缩放、主题切换）。
- 热键看门狗、心跳离线检测的 UI 联动。
- 图标资源、单文件体积与内存实测、打包说明。

## 实测数据（CI，run #6）

- 工具链：Rust 1.77.2 x86_64-pc-windows-msvc
- Release 二进制体积：约 95–175 KB（远低于 15 MB 目标）
- 单元测试：16 个全部通过
- CI：GitHub Actions windows-latest，构建 + 测试 + 产物上传全绿

## 修改记录

### 2026-09-25

- 安装项目内 Rust 1.77.2 便携工具链。
- 建立 Cargo 项目骨架与 CI 构建管线。
- 实现 config、core（title/net）、network、platform/autostart，附单元测试。
