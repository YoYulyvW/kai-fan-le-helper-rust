# 打包与发布说明

## 工具链

- Rust 1.77.2（由 `rust-toolchain.toml` 固定，最后一个官方支持 Windows 7 的版本）
- 目标：`x86_64-pc-windows-msvc`

## 本地编译（需 MSVC 链接器）

```bat
:: 使用项目内便携工具链
set RUSTUP_HOME=%CD%\.rustup
set CARGO_HOME=%CD%\.cargo
set PATH=%CD%\.cargo\bin;%PATH%

cargo build --release
```

产物：`target/release/kai-fan-le-helper.exe`

> MSVC 链接器来自 Visual Studio Build Tools（“使用 C++ 的桌面开发”工作负载）。
> 若本机未安装，可用 GitHub Actions 编译（见下）。

## GitHub Actions（推荐）

推送到 `main` 即自动构建、测试并上传产物：

```
Actions → build → Artifacts → kai-fan-le-helper-windows
```

CI 使用 `windows-latest` runner（自带 MSVC），无需本地环境。

## 体积优化

`Cargo.toml` 的 release profile 已启用：

```toml
[profile.release]
opt-level = "z"     # 最小体积
lto = true          # 链接时优化
codegen-units = 1   # 单编译单元，利于优化
panic = "abort"     # 去 panic 展开，减小体积
strip = true        # 去符号表
```

实测 Release 二进制约 95–175 KB，无需额外打包即可单文件分发。

## Windows 7 兼容

- 使用 Rust 1.77.2 编译，产物兼容 Windows 7 SP1 及以上。
- 未使用 Win8+ 专有 API；托盘使用 Shell_NotifyIcon（Win7 可用）。
- 热键钩子 WH_KEYBOARD_LL 兼容 Win7。

## 运行依赖

- 无第三方运行时依赖（无 .NET、无 VC++ 运行库额外要求，MSVC 运行时为系统自带）。
- 配置文件自动生成于用户目录。
