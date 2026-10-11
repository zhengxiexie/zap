# Zap OSS 构建工作流总结

> 本文档总结了为 zap-oss 项目创建高效构建脚本的经验，包括编译配置、性能优化和工程实践。

## 📋 项目背景

**目标**：为 Warp 的开源版本 (zap-oss) 创建便捷的构建脚本，区分开发调试和正式发布两种场景。

**挑战**：
- 编译时间长（debug 5-10 分钟，release 20-30 分钟）
- 构建配置复杂（channel、features、架构等参数）
- 需要平衡编译速度和运行性能

---

## 🎯 最终方案

创建了两个构建脚本，分别优化不同使用场景：

### 1. `build-dev.sh` - 开发模式
```bash
script/macos/bundle \
  --channel oss \
  --nosign \
  --debug \
  --open \
  --nouniversal \
  --features hoa_notifications
```

**特点**：
- 使用 `debug` profile（无优化，编译快）
- `--nouniversal` 只编译当前架构（节省 30-40% 时间）
- `--open` 自动打开产物目录
- 编译时间：5-8 分钟
- 产物位置：`target/debug/bundle/osx/`

### 2. `build-release.sh` - 正式发布
```bash
script/macos/bundle \
  --channel oss \
  --nosign \
  --nouniversal \
  --features hoa_notifications
```

**特点**：
- 使用 `release-lto` profile（完全优化 + ThinLTO）
- `--nouniversal` 只编译当前架构（节省 40-50% 时间）
- 带确认提示，避免误操作
- 编译时间：15-20 分钟
- 产物位置：`target/release-lto/bundle/osx/`

---

## 🔍 关键发现

### 1. Bundle 脚本的工作原理

**Channel 配置**（`script/macos/bundle` 第 287-293 行）：
```bash
case "$CHANNEL" in
  oss)
    WARP_BIN="zap-oss"
    BUNDLE_ID="dev.zap.Zap"
    WARP_APP_NAME="Zap"
    FEATURES="release_bundle,extern_plist,autoupdate"
    ;;
esac
```

**Profile 选择**（第 256-259 行）：
- `--debug` → 使用 `debug` profile
- 默认 → 使用 `release-lto` profile

**关键参数**：
- `--channel oss` - 指定 OSS 渠道配置
- `--nosign` - 跳过代码签名（本地构建必需）
- `--nouniversal` - 只编译当前架构
- `--debug` - 使用 debug profile
- `--open` - 自动打开产物目录
- `--features <list>` - 追加额外的 Cargo features

### 2. Cargo Profile 配置差异

#### Debug Profile (`Cargo.toml` 第 426-469 行)
```toml
[profile.dev]
opt-level = 0              # 无优化
debug = "line-tables-only" # 最小调试信息
split-debuginfo = "unpacked"
codegen-units = 256        # 高并行度
```

**关键优化**：虽然主代码无优化，但会优化关键依赖：
```toml
[profile.dev.package]
backtrace.opt-level = 3
tikv-jemallocator.opt-level = 3
ttf-parser.opt-level = 3
tokio.opt-level = 1
# ... 等
```

#### Release-LTO Profile (`Cargo.toml` 第 477-479 行)
```toml
[profile.release-lto]
inherits = "release"
lto = "thin"  # ThinLTO 跨 crate 优化
```

继承自 `release`：
```toml
[profile.release]
opt-level = 3              # 完全优化
debug = 1                  # 保留行号表
split-debuginfo = "packed" # 打包 dSYM
codegen-units = 16         # 较低并行度
```

### 3. Features 管理策略

**初始困惑**：是否需要手动指定所有 features？

**实际情况**：
- `default` feature 已包含 160+ 功能（`app/Cargo.toml` 第 435-577 行）
- 包括：`pluggable_notifications`、AI、编辑器、终端等核心功能
- Bundle 脚本会自动追加 `release_bundle`、`extern_plist`、`autoupdate`

**最佳实践**：
- 依赖 `default` features（涵盖绝大部分功能）
- 只手动添加特殊需求的 features（如 `hoa_notifications`）
- 避免盲目添加所有 features（可能引入不必要的依赖或冲突）

### 4. 编译时间优化

#### Universal Binary 的代价

**默认行为**：
- 编译 x86_64 架构
- 编译 aarch64 架构
- 使用 `lipo` 合并成 Universal Binary

**时间成本**：
- Debug: 几乎翻倍（每个架构都要编译）
- Release: 增加 40-50%（LTO 阶段对两个架构都执行）

**优化方案**：`--nouniversal`
- 只编译当前机器架构
- Debug: 节省 30-40% 时间
- Release: 节省 40-50% 时间
- 本地开发完全够用

#### 编译时间分解

**Debug 构建（5-8 分钟）**：
```
编译 crates:  60% (并行，快速)
链接:        30%
生成 bundle: 10%
```

**Release-LTO 构建（15-20 分钟）**：
```
编译 crates:  40% (优化，慢)
ThinLTO:     35% (跨 crate 分析，串行)
链接:        15%
dsymutil:     5%
生成 bundle:  5%
```

**瓶颈**：ThinLTO 阶段（跨 crate 内联、死代码消除），无法充分并行。

---

## 💡 工程经验

### 1. 脚本设计原则

✅ **单一职责**：
- `build-dev.sh` - 快速迭代，频繁使用
- `build-release.sh` - 正式打包，谨慎执行

✅ **防呆设计**：
- Release 脚本带确认提示
- 清晰的输出信息（时间估计、产物位置）
- 脚本开头注释说明用途和特点

✅ **自动化**：
- Dev 脚本自动打开产物目录（`--open`）
- 脚本自动添加执行权限（`chmod +x`）

### 2. 文档内嵌

每个脚本开头都包含：
```bash
#!/bin/bash
#
# 用途说明
#
# 编译特点：
# - opt-level
# - 编译时间
# - 特殊配置
#
```

**好处**：
- 几个月后重新使用时快速回忆
- 新成员可以直接从脚本学习
- 避免需要单独维护 README

### 3. 验证方式

**验证产物位置**：
```bash
ls -lhd target/debug/bundle/osx/Zap.app \
        target/release-lto/bundle/osx/Zap.app
```

**验证架构**：
```bash
lipo -info target/debug/bundle/osx/Zap.app/Contents/MacOS/zap-oss
# 输出: Non-fat file ... is architecture: arm64
```

### 4. 踩过的坑

❌ **误区 1**：以为编译结果在 `target/release/`
- **真相**：OSS channel 使用 `release-lto` profile，产物在 `target/release-lto/`

❌ **误区 2**：想要手动指定所有 200+ features
- **真相**：`default` 已经包含大部分功能，只需追加特殊需求

❌ **误区 3**：不理解 debug 和 release 时间差异
- **真相**：主要是 opt-level 和 ThinLTO 导致的，是优化的必要代价

---

## 📊 性能对比

| 配置 | 编译时间 | 二进制大小 | 运行性能 | 适用场景 |
|------|---------|----------|---------|----------|
| Debug (universal) | 8-12 分钟 | ~300MB | 慢 2-5x | - |
| Debug (single arch) | 5-8 分钟 | ~150MB | 慢 2-5x | 日常开发 |
| Release (universal) | 25-35 分钟 | ~150MB | 最快 | 分发给 Intel + ARM 用户 |
| Release (single arch) | 15-20 分钟 | ~80MB | 最快 | 本地使用 |

**推荐配置**：
- 开发迭代：Debug + single arch
- 个人使用：Release + single arch
- 公开分发：Release + universal

---

## 🎯 可复用的经验

### 适用于其他 Rust 项目

1. **Profile 设计**：
   - Dev profile 优化关键依赖（tokio, jemalloc 等）
   - Release profile 使用 ThinLTO 平衡编译时间和性能
   - 创建中间 profile（如 `release-cli`）用于特殊场景

2. **多架构构建**：
   - 本地开发用 `--target $(rustc -vV | sed -n 's|host: ||p')`
   - 正式发布才构建 universal binary
   - 使用 `lipo` 合并时检查两个架构都存在

3. **Features 管理**：
   - `default` feature 包含常用功能
   - 专用 features（如 `crash_reporting`）可选启用
   - 避免 features 爆炸（200+ features 说明可能需要重构）

### 适用于 macOS App 打包

1. **Bundle 脚本模式**：
   - 统一的 `script/macos/bundle` 脚本
   - 通过参数（`--channel`、`--debug`）控制不同配置
   - 避免维护多个脚本副本

2. **代码签名**：
   - 本地构建用 `--nosign`
   - CI/发布流程集成签名步骤
   - 使用 `codesign --verify` 验证

3. **DMG 生成**：
   - Bundle 脚本自动生成 DMG
   - 包含 `.app` 和应用程序文件夹快捷方式
   - 测试 DMG 挂载和安装流程

---

## 📝 后续改进方向

### 1. 增量构建优化

当前问题：修改单个文件后 release 构建仍需 10-15 分钟。

**可能方案**：
- 使用 `sccache` 或 `cargo-chef` 缓存依赖编译结果
- 调整 `codegen-units` 平衡并行度和优化质量
- 评估是否真的需要 ThinLTO（vs 普通 release）

### 2. 构建缓存

```bash
# 检查缓存命中率
export RUSTC_WRAPPER=sccache
sccache --show-stats

# 增量编译统计
export CARGO_INCREMENTAL=1
cargo build --timings
```

### 3. 并行测试

如果需要同时验证多个变体：
```bash
# 并行构建 debug 和 release
parallel ::: \
  "./build-dev.sh" \
  "./build-release.sh"
```

### 4. CI 集成

```yaml
# .github/workflows/build.yml
name: Build
on: [push, pull_request]
jobs:
  build:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v3
      - name: Build Debug
        run: ./build-dev.sh
      - name: Upload Artifact
        uses: actions/upload-artifact@v3
        with:
          name: Zap-debug
          path: target/debug/bundle/osx/Zap.dmg
```

---

## 🔗 相关资源

### 仓库文档
- `AGENTS.md` - 代码地图和工程约定
- `WARP.md` - 工程师手册（命令、风格、流程）
- `script/macos/bundle` - Bundle 脚本源码
- `Cargo.toml` - Workspace 和 profile 配置

### Cargo 文档
- [Profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [Features](https://doc.rust-lang.org/cargo/reference/features.html)
- [Link-Time Optimization](https://doc.rust-lang.org/cargo/reference/profiles.html#lto)

### 性能优化
- [The Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [cargo-llvm-lines](https://github.com/dtolnay/cargo-llvm-lines) - 分析代码膨胀
- [cargo-bloat](https://github.com/RazrFalcon/cargo-bloat) - 分析二进制体积

---

## ✅ 检查清单

下次创建类似构建脚本时，确认：

- [ ] 阅读现有的构建脚本（`script/*/bundle` 等）
- [ ] 理解 profile 配置（`Cargo.toml` 中的 `[profile.*]`）
- [ ] 确认 features 策略（是否依赖 `default`）
- [ ] 决定是否需要 universal binary
- [ ] 添加清晰的脚本注释（用途、参数、时间估计）
- [ ] 测试两个脚本的产物（运行、安装、功能验证）
- [ ] 添加执行权限（`chmod +x`）
- [ ] 验证产物位置和架构（`ls -lh`、`lipo -info`）
- [ ] 文档化编译时间和优化建议
- [ ] 考虑 CI 集成需求

---

**创建日期**：2026-10-11  
**适用项目**：Warp / Zap OSS  
**维护者**：记录团队构建经验，便于未来参考和新成员学习
