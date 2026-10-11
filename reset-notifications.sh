#!/bin/bash
# 重置 Zap 的通知权限
# 解决 Bundle ID 相同但签名不同导致的通知权限混淆问题

set -e

echo "🔔 重置 Zap 通知权限"
echo "=================="
echo ""

# 1. 关闭所有 Zap 进程
if pgrep -f "Zap.app/Contents/MacOS/zap-oss" > /dev/null; then
    echo "🛑 关闭正在运行的 Zap..."
    killall zap-oss 2>/dev/null || true
    sleep 2
    echo "✅ Zap 已关闭"
else
    echo "✅ 没有运行中的 Zap 进程"
fi

echo ""

# 2. 清理应用状态（包括通知权限缓存）
echo "🗑️  清理应用状态..."
rm -rf ~/Library/Saved\ Application\ State/dev.zap.Zap.savedState 2>/dev/null || true
rm -rf ~/Library/Preferences/dev.zap.Zap.plist 2>/dev/null || true
echo "✅ 应用状态已清理"

echo ""

# 3. 重置系统通知数据库（需要重启 macOS 通知中心）
echo "🔄 重置通知数据库..."
echo "⚠️  这将重启 macOS 通知中心服务"

# 删除 Zap 在通知数据库中的记录
defaults delete com.apple.ncprefs.plist "apps" 2>/dev/null || true

# 重启通知中心
killall NotificationCenter 2>/dev/null || true
killall usernoted 2>/dev/null || true

sleep 2
echo "✅ 通知数据库已重置"

echo ""
echo "✨ 重置完成！"
echo ""
echo "📝 下一步："
echo "1. 运行 ./build-arm64.sh 重新编译并安装 Zap"
echo "2. 首次启动时，系统会弹出通知权限请求对话框"
echo "3. 点击【允许】授予通知权限"
echo ""
