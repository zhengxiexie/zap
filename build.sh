#!/bin/bash
#
# 统一构建脚本
# 用法：
#   ./build.sh dev              # 开发版（debug，快速编译）
#   ./build.sh arm64            # ARM64 版本（debug，自动安装）
#   ./build.sh release          # 正式发布版（release-lto，完全优化）
#

set -e

# 始终以仓库根目录为工作目录，避免在 iTerm2 任意路径下执行时找不到 bundle 脚本
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

# 本地 ad-hoc 签名用的 entitlements
ENTITLEMENTS="$REPO_ROOT/script/Debug-Entitlements.plist"

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

# 公共函数：确保 app bundle 的签名有效
#
# macOS 只给「签名绑定 Info.plist」的 app 发放通知权限。没有 _CodeSignature、
# 或 Identifier 与 CFBundleIdentifier 不一致时，usernotificationsd 会直接拒绝：
#   Entitlement 'com.apple.private.usernotifications.bundle-identifiers' required
# 所以每次编译完都在本地 ad-hoc 重签一遍。
ensure_signed() {
    local app_path="$1"
    local expected_id identifier

    expected_id="$(plutil -extract CFBundleIdentifier raw "$app_path/Contents/Info.plist" 2>/dev/null || true)"

    if codesign --verify --deep "$app_path" > /dev/null 2>&1; then
        identifier="$(codesign -dv "$app_path" 2>&1 | awk -F= '/^Identifier=/{print $2}')"
        if [ -z "$expected_id" ] || [ "$identifier" = "$expected_id" ]; then
            echo "✅ 签名有效: $app_path (Identifier=$identifier)"
            return 0
        fi
        echo "⚠️  签名 Identifier 与 bundle 不一致 ($identifier != $expected_id)，重新签名..."
    else
        echo "⚠️  签名无效或缺失，重新签名..."
    fi

    # 先清掉扩展属性，否则 codesign 会报
    # "resource fork, Finder information, or similar detritus not allowed"
    xattr -cr "$app_path"
    codesign --force --deep --options runtime --sign - \
        --entitlements "$ENTITLEMENTS" "$app_path"
    codesign --verify --deep "$app_path"

    identifier="$(codesign -dv "$app_path" 2>&1 | awk -F= '/^Identifier=/{print $2}')"
    echo "✅ 签名完成: $app_path (Identifier=$identifier)"
}

# 公共函数：安装到 Applications
install_to_applications() {
    local source_path="$1"

    echo "📦 安装到 /Applications/Zap.app..."
    rm -rf /Applications/Zap.app
    # 用 ditto 而不是 cp -R，完整保留签名和扩展属性
    ditto "$source_path" /Applications/Zap.app

    ensure_signed /Applications/Zap.app

    echo "🚀 启动 Zap..."
    open /Applications/Zap.app

    echo "✨ 完成！Zap 已重新启动"
    echo ""
    echo "⚠️  若系统设置 > 通知里没有 Zap，或通知不弹，说明签名变了导致授权被重置："
    echo "    重新在 Zap 里触发一次通知即可看到授权弹窗，或到系统设置手动开启"
}

# 检查参数
if [ $# -eq 0 ]; then
    echo "${RED}错误：缺少构建类型参数${NC}"
    echo ""
    echo "用法："
    echo "  ./build.sh dev       # 开发版（5-10分钟，终端运行查看日志）"
    echo "  ./build.sh release   # 发布版（20-30分钟，完全优化，自动安装）"
    echo "  ./build.sh sign <Zap.app>  # 仅对已存在的 app bundle 校验/补签名"
    exit 1
fi

BUILD_TYPE="$1"

case "$BUILD_TYPE" in
    sign)
        if [ -z "$2" ]; then
            echo "${RED}错误：缺少 app 路径${NC}"
            echo "用法: ./build.sh sign /path/to/Zap.app"
            exit 1
        fi
        ensure_signed "$2"
        ;;

    dev)
        echo "🔨 开始编译 zap-oss 开发版本..."
        echo "使用 debug profile，启用快速编译"
        echo "使用 default features（包含 160+ 功能）"
        echo ""
        
        script/macos/bundle \
          --channel oss \
          --selfsign \
          --debug \
          --nouniversal \
          --features hoa_notifications
        
        echo ""
        echo "${GREEN}✅ 编译完成！${NC}"
        echo "📦 产物位置: target/debug/bundle/osx/Zap.app"
        echo ""

        ensure_signed target/debug/bundle/osx/Zap.app

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
          --selfsign \
          --nouniversal \
          --features hoa_notifications
        
        echo ""
        echo "${GREEN}✅ 编译完成！${NC}"
        echo "📦 DMG 位置: target/release-lto/bundle/osx/Zap.dmg"
        echo "📱 应用位置: target/release-lto/bundle/osx/Zap.app"
        echo ""
        
        # 自动安装
        stop_zap
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
