# 修复 macOS 通知权限问题

## 问题描述

Zap 应用显示错误提示：「Zap 没有发送桌面通知的权限」

用户在 macOS 系统设置 > 通知 中找不到 Zap 应用，无法手动授予权限。

## 根本原因

Zap 应用的 `Info.plist` 文件中缺少 `NSUserNotificationUsageDescription` 键，导致：

1. macOS 不会为 Zap 创建通知权限条目
2. Zap 应用无法请求通知权限
3. 系统设置中不显示 Zap 的通知选项

## 诊断过程

### 1. 确认问题范围

```bash
# 查找 Zap 应用位置
mdfind "kMDItemKind == 'Application'" | rg -i "zap"
# 结果：/Applications/Zap.app

# 检查 bundle identifier
defaults read /Applications/Zap.app/Contents/Info.plist CFBundleIdentifier
# 结果：dev.zap.Zap
```

### 2. 验证通知机制

```bash
# 测试系统通知功能（成功）
osascript -e 'display notification "测试通知" with title "Zap"'

# 尝试通过 Zap 应用发送通知（无权限）
osascript -e 'tell application "Zap" to display notification "测试" with title "Zap"'
```

### 3. 检查 Info.plist 配置

查看 `/Applications/Zap.app/Contents/Info.plist`，发现：

- ✅ 有 `NSCameraUsageDescription`
- ✅ 有 `NSMicrophoneUsageDescription`
- ✅ 有 `NSContactsUsageDescription`
- ✅ 有 `NSPhotoLibraryUsageDescription`
- ❌ **缺少 `NSUserNotificationUsageDescription`**

## 解决方案

### 方案 A：源码级修复（推荐）

这是**最佳方案**，直接在构建脚本中添加通知权限声明，一劳永逸。

#### 修改 script/update_plist

在 `/Users/zhengxie/Code/zap/script/update_plist` 文件中，找到权限声明部分并添加通知权限：

```bash
echo "Updating plist with permissions descriptions"
plutil -insert NSAppleEventsUsageDescription -string "A program in Warp wants to use AppleScript." "$WARP_PLIST_PATH"
plutil -insert NSCameraUsageDescription -string "A program in Warp wants to use the camera." "$WARP_PLIST_PATH"
plutil -insert NSMicrophoneUsageDescription -string "A program in Warp wants to use your microphone." "$WARP_PLIST_PATH"
plutil -insert NSContactsUsageDescription -string "A program in Warp wants to use your contacts." "$WARP_PLIST_PATH"
plutil -insert NSCalendarsUsageDescription -string "A program in Warp wants to use your calendar." "$WARP_PLIST_PATH"
plutil -insert NSLocationUsageDescription -string "A program in Warp wants to use your location information." "$WARP_PLIST_PATH"
plutil -insert NSPhotoLibraryUsageDescription -string "A program in Warp wants to use your photo library." "$WARP_PLIST_PATH"
# 添加这一行 ↓
plutil -insert NSUserNotificationUsageDescription -string "Zap wants to send you notifications about task completion and important updates." "$WARP_PLIST_PATH"
```

#### 重新构建 Zap

修改完成后，使用任一构建脚本重新编译：

```bash
# 开发版本（快速编译，5-10 分钟）
./build-dev.sh

# ARM64 版本（适用于 Apple Silicon）
./build-arm64.sh

# Release 版本（完全优化，20-30 分钟）
./build-release.sh
```

构建脚本会自动：
1. 调用 `script/update_plist` 添加所有权限声明
2. 编译应用
3. 安装到 `/Applications/Zap.app`
4. 自动启动 Zap

首次启动时，macOS 会弹出通知权限请求，点击「允许」即可。

**优点：**
- ✅ 永久性修复，不需要重复操作
- ✅ 每次重新编译都会自动包含通知权限
- ✅ 不破坏代码签名
- ✅ 符合正规开发流程

---

### 方案 B：临时手动修复（已安装应用）

如果已经安装了 Zap 但不想重新编译，可以手动修改已安装应用的 Info.plist。

⚠️ **注意**：此方法需要重新签名，且每次更新 Zap 都会失效。

### 步骤 1：备份原始文件

```bash
cp /Applications/Zap.app/Contents/Info.plist /Applications/Zap.app/Contents/Info.plist.backup
```

### 步骤 2：修改 Info.plist

在 `NSPhotoLibraryUsageDescription` 后添加：

```xml
<key>NSUserNotificationUsageDescription</key>
<string>Zap wants to send you notifications about task completion and important updates.</string>
```

完整上下文：

```xml
<key>NSPhotoLibraryUsageDescription</key>
<string>A program in Warp wants to use your photo library.</string>
<key>NSUserNotificationUsageDescription</key>
<string>Zap wants to send you notifications about task completion and important updates.</string>
<key>NSServices</key>
```

### 步骤 3：重新签名应用

修改 Info.plist 后必须重新签名，否则 macOS 会拒绝运行：

```bash
# 重新签名（使用临时签名）
codesign -s - --force --deep /Applications/Zap.app

# 如果遇到 "resource fork, Finder information" 错误，先清理扩展属性
xattr -cr /Applications/Zap.app
codesign -s - --force --deep /Applications/Zap.app
```

### 步骤 4：重启 Zap

```bash
# 完全退出 Zap
killall Zap

# 或者使用 ⌘Q 退出 Zap

# 重新启动 Zap
open /Applications/Zap.app
```

### 步骤 5：授予通知权限

重启 Zap 后：

1. 当 Zap 首次尝试发送通知时，macOS 会弹出权限请求对话框
2. 点击「允许」
3. 或者在「系统设置 > 通知」中找到 Zap 并手动开启

## 验证结果

修复后，用户应该能够：

- ✅ 在系统设置 > 通知中看到 Zap 应用
- ✅ 收到 Zap 的桌面通知
- ✅ 自定义 Zap 的通知样式（横幅、提醒、声音等）

## 注意事项

### 临时解决方案的局限性

这是一个**临时修复**，存在以下问题：

1. **更新覆盖**：每次 Zap 更新都会覆盖修改，需要重新操作
2. **签名警告**：使用临时签名可能触发 macOS 的安全警告
3. **非官方修改**：修改了应用包内容，可能影响后续官方支持

### 长期解决方案

应该由 Zap 开发团队在构建配置中添加此权限声明：

**构建脚本或 Info.plist 模板中添加：**

```xml
<key>NSUserNotificationUsageDescription</key>
<string>Zap wants to send you notifications about task completion and important updates.</string>
```

## 相关文件

- `/Applications/Zap.app/Contents/Info.plist` - 应用配置文件
- `/Applications/Zap.app/Contents/Info.plist.backup` - 备份文件

## 相关技术

- macOS Notification Center
- `NSUserNotificationUsageDescription` - 通知权限声明键
- Code Signing - macOS 应用签名机制
- TCC (Transparency, Consent, and Control) - macOS 权限管理系统

## 参考资源

- [Apple - Requesting Authorization to Interact with the User](https://developer.apple.com/documentation/usernotifications/asking_permission_to_use_notifications)
- [Info.plist Keys - NSUserNotificationUsageDescription](https://developer.apple.com/documentation/bundleresources/information_property_list)
- [Code Signing Guide](https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/)

## 日期

- 问题发现：2026-10-11
- 修复完成：2026-10-11
- 文档创建：2026-10-11
