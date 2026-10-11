#!/bin/bash
#
# 正式打包：编译 release 优化版本的 DMG
# 用于生成可安装分发的 DMG 包
#
# 编译特点：
# - 完全优化 (opt-level = 3)
# - ThinLTO 跨 crate 优化
# - 打包调试符号 (dSYM)
# - 编译时间：20-30 分钟（首次），10-15 分钟（增量）
#

set -e

echo "📦 开始编译 zap-oss 正式发布版本..."
echo "使用 release-lto profile，完全优化编译"
echo "使用 default features（包含 160+ 功能）"
echo "⚠️  这将需要较长时间（首次编译可能需要 20-30 分钟）"
echo ""

read -p "确认继续? [y/N] " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "已取消"
    exit 1
fi

echo "🔨 开始编译..."
echo ""

# 编译 release 版本的 DMG
# 使用 default features + hoa_notifications
# 只编译当前架构（Apple Silicon），跳过 x86_64
script/macos/bundle \
  --channel oss \
  --nosign \
  --nouniversal \
  --features hoa_notifications

echo ""
echo "✅ 编译完成！"
echo "📦 DMG 位置: target/release-lto/bundle/osx/Zap.dmg"
echo "📱 应用位置: target/release-lto/bundle/osx/Zap.app"
echo ""
echo "🎯 可以用以下命令安装:"
echo "   open target/release-lto/bundle/osx/Zap.dmg"
echo ""
echo "或者直接运行应用:"
echo "   open target/release-lto/bundle/osx/Zap.app"
