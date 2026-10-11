#!/bin/bash
#
# 统一构建脚本
# 用法：
#   ./build.sh dev              # 开发版（debug，快速编译）
#   ./build.sh arm64            # ARM64 版本（debug，自动安装）
#   ./build.sh release          # 正式发布版（release-lto，完全优化）
#

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 公共函数：停止运行中的 Zap
stop_zap() {
    if pgrep -f "Zap.app/Contents/MacOS/zap-oss" > /dev/null; then
        echo "🛑 关闭正在运行的 Zap..."
        killall zap-oss 2>/dev/null || true
        sleep 1
    fi
}

# 公共函数：重置通知权限
reset_notification_permission() {
    echo "🔔 重置通知权限..."
    rm -rf ~/Library/Saved\ Application\ State/dev.zap.Zap.savedState 2>/dev/null || true
}

# 公共函数：安装到 Applications
install_to_applications() {
    local source_path="$1"
    
    echo "📦 安装到 /Applications/Zap.app..."
    rm -rf /Applications/Zap.app
    cp -R "$source_path" /Applications/
    
    echo "🚀 启动 Zap..."
    open /Applications/Zap.app
    
    echo "✨ 完成！Zap 已重新启动"
    echo ""
    echo "⚠️  首次启动时，系统会弹出通知权限请求对话框，请点击【允许】"
}

# 检查参数
if [ $# -eq 0 ]; then
    echo "${RED}错误：缺少构建类型参数${NC}"
    echo ""
    echo "用法："
    echo "  ./build.sh dev       # 开发版（5-10分钟，终端运行查看日志）"
    echo "  ./build.sh release   # 发布版（20-30分钟，完全优化，自动安装）"
    exit 1
fi

BUILD_TYPE="$1"

case "$BUILD_TYPE" in
    dev)
        echo "🔨 开始编译 zap-oss 开发版本..."
        echo "使用 debug profile，启用快速编译"
        echo "使用 default features（包含 160+ 功能）"
        echo ""
        
        script/macos/bundle \
          --channel oss \
          --nosign \
          --debug \
          --nouniversal \
          --features hoa_notifications
        
        echo ""
        echo "${GREEN}✅ 编译完成！${NC}"
        echo "📦 产物位置: target/debug/bundle/osx/Zap.app"
        echo ""
        
        # 停止运行中的 Zap
        stop_zap
        
        echo "🚀 启动 zap-oss 二进制（终端模式，可查看日志）..."
        echo "按 Ctrl+C 停止"
        echo ""
        
        # 直接运行二进制，输出到当前终端
        exec target/debug/bundle/osx/Zap.app/Contents/MacOS/zap-oss
        ;;
    
    release)
        echo "📦 开始编译 zap-oss 正式发布版本..."
        echo "使用 release-lto profile，完全优化编译"
        echo "使用 default features（包含 160+ 功能）"
        echo "${YELLOW}⚠️  这将需要较长时间（首次编译可能需要 20-30 分钟）${NC}"
        echo ""
        
        read -p "确认继续? [y/N] " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            echo "已取消"
            exit 1
        fi
        
        echo "🔨 开始编译..."
        echo ""
        
        script/macos/bundle \
          --channel oss \
          --nosign \
          --nouniversal \
          --features hoa_notifications
        
        echo ""
        echo "${GREEN}✅ 编译完成！${NC}"
        echo "📦 DMG 位置: target/release-lto/bundle/osx/Zap.dmg"
        echo "📱 应用位置: target/release-lto/bundle/osx/Zap.app"
        echo ""
        
        # 自动安装
        stop_zap
        reset_notification_permission
        install_to_applications "target/release-lto/bundle/osx/Zap.app"
        ;;
    
    *)
        echo "${RED}错误：未知的构建类型 '$BUILD_TYPE'${NC}"
        echo ""
        echo "支持的类型："
        echo "  dev      # 开发版（终端运行，查看日志）"
        echo "  release  # 发布版（自动安装到 Applications）"
        exit 1
        ;;
esac
