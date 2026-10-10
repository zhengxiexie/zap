#!/bin/bash
# arm64 架构编译脚本

set -e  # 遇到错误立即退出

echo "🔨 开始编译 Zap (arm64)..."
./script/macos/bundle --selfsign --nosign --arch aarch64 -c oss --features hoa_notifications

echo "✅ 编译完成"

# 检查是否有正在运行的 Zap 进程
if pgrep -f "Zap.app/Contents/MacOS/zap-oss" > /dev/null; then
    echo "🛑 关闭正在运行的 Zap..."
    killall zap-oss 2>/dev/null || true
    sleep 1
fi

# 安装到 Applications
echo "📦 安装到 /Applications/Zap.app..."
rm -rf /Applications/Zap.app
cp -R target/debug/bundle/osx/Zap.app /Applications/

echo "🚀 启动 Zap..."
open /Applications/Zap.app

echo "✨ 完成！Zap 已重新启动"
