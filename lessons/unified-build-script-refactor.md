# 统一构建脚本重构

## 背景

原有三个构建脚本（`build-dev.sh`, `build-arm64.sh`, `build-release.sh`）存在大量重复代码（约60%），维护成本高。

## 问题分析

### 重复代码

三个脚本都包含以下相同逻辑：
- 检查并停止运行中的 Zap 进程
- 重置通知权限（清理 savedState）
- 安装到 `/Applications/Zap.app`
- 启动 Zap 应用

### 主要区别

| 脚本 | 编译模式 | 特点 |
|------|---------|------|
| `build-dev.sh` | debug | 自动打开目录，不自动安装 |
| `build-arm64.sh` | debug (arm64) | 指定架构，自动安装 |
| `build-release.sh` | release-lto | 交互式确认，可选安装 |

实际上 `build-arm64.sh` 和 `build-dev.sh` 都是 debug 模式，只是一个指定了架构。而 `build-release.sh` 本身就用了 `--nouniversal` 参数，只编译当前架构（arm64），所以 `build-arm64.sh` 是多余的。

## 解决方案

### 统一的 build.sh

提取公共函数，支持两种模式：dev（开发调试）和 release（发布版本）。

### 模式设计

#### dev 模式
```bash
./build.sh dev
```

- **用途**：日常开发调试
- **编译时间**：5-10 分钟
- **行为**：直接在终端运行二进制，实时查看日志

#### release 模式
```bash
./build.sh release
```

- **用途**：生成可发布的优化版本
- **编译时间**：20-30 分钟（首次）
- **行为**：自动安装到 /Applications 并启动

## 关键改进

1. **dev 模式不再打开 Finder**，改为直接在终端运行二进制，方便查看日志
2. **release 模式自动安装**，减少手动操作
3. **消除 60% 重复代码**，提取公共函数

## 优势总结

- ✅ dev 模式更适合调试（实时日志）
- ✅ release 模式更省心（自动安装）
- ✅ 统一入口，易于维护

## 与其他修复的关联

配合 [fix-macos-notification-permission.md](./fix-macos-notification-permission.md) 中的源码修复，所有构建模式都会自动包含通知权限声明。

## 相关文件

- `/Users/zhengxie/Code/zap/build.sh` - 统一构建脚本
- `/Users/zhengxie/Code/zap/README.md` - 英文文档
- `/Users/zhengxie/Code/zap/README.zh-CN.md` - 中文文档

## 日期

- 重构完成：2026-10-11
- 文档创建：2026-10-11
