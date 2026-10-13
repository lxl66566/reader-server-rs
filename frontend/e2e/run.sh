#!/usr/bin/env bash
# 运行 E2E 测试：NixOS 上 chromium 缺系统动态库，需经 nix-ld 提供依赖路径；
# 非 NixOS（无 nix-build）直接执行 playwright test。
#
# 目标服务器默认 http://127.0.0.1:3000（release 内嵌前端），可用
# E2E_BASE_URL 覆盖，例如对本地 vite 开发服务器跑：
#   pnpm dev &   # vite 监听 5173，已配置 /api、ws 代理到 3000
#   E2E_BASE_URL=http://127.0.0.1:5173 pnpm e2e
set -euo pipefail
cd "$(dirname "$0")/.."

# 准备 UTF-8 测试书：仓库里的 2.txt 是 GBK 编码（服务端按设计拒绝非
# UTF-8 txt），转换副本放 /tmp，不入库。
book="/tmp/reader-e2e-2.txt"
if [ ! -f "$book" ] && [ -f ../2.txt ]; then
  iconv -f GB18030 -t UTF-8 ../2.txt > "$book"
fi

if command -v nix-build >/dev/null 2>&1; then
  deps="/tmp/reader-e2e-chromium-deps"
  nix-build e2e/chromium-deps.nix -o "$deps" >/dev/null
  export NIX_LD_LIBRARY_PATH="$deps/lib${NIX_LD_LIBRARY_PATH:+:$NIX_LD_LIBRARY_PATH}"
fi
exec npx playwright test "$@"
