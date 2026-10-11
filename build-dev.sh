#!/bin/bash
#
# 开发模式：快速编译 debug 版本并自动打开应用
# 用于日常开发调试
#
# 编译特点：
# - 无优化 (opt-level = 0)
# - 只优化关键依赖 (tokio, jemalloc 等)
# - 增量编译友好
# - 编译时间：5-10 分钟
#

set -e

echo "🔨 开始编译 zap-oss 开发版本..."
echo "使用 debug profile，启用快速编译"
echo "使用 default features（包含 160+ 功能）"
echo ""

# 编译 debug 版本的 DMG
# 使用 default features + hoa_notifications
# 只编译当前架构（Apple Silicon），跳过 x86_64
script/macos/bundle \
  --channel oss \
  --nosign \
  --debug \
  --open \
  --nouniversal \
  --features hoa_notifications

echo ""
echo "✅ 编译完成！"
echo "📦 产物位置: target/debug/bundle/osx/"
echo "🚀 应用已自动打开所在目录"
echo ""
echo "可以直接运行: target/debug/bundle/osx/Zap.app"
