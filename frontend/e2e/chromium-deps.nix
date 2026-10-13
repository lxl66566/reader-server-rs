# chromium 运行时依赖合集：供 NixOS 上通过 nix-ld 启动 Playwright 的
# chrome-headless-shell（非 NixOS 不需要，见 e2e/run.sh）。
# 用法：nix-build e2e/chromium-deps.nix -o /tmp/reader-e2e-chromium-deps
#       export NIX_LD_LIBRARY_PATH=/tmp/reader-e2e-chromium-deps/lib:$NIX_LD_LIBRARY_PATH
with import <nixpkgs> {};
let
  # 运行时库所在输出（部分包的库在单独的 lib 输出里，out 只有可执行文件）
  pick =
    p:
    builtins.filter (x: x != null) [
      (p.out or p)
      (p.lib or null)
    ];
in
buildEnv {
  name = "chromium-headless-deps";
  paths = builtins.concatLists (map pick [
    glib
    nss
    nspr
    dbus
    expat
    systemd
    libudev-zero
    libdrm
    mesa
    libgbm
    libxkbcommon
    xorg.libX11
    xorg.libxcb
    xorg.libXcomposite
    xorg.libXdamage
    xorg.libXext
    xorg.libXfixes
    xorg.libXrandr
    alsa-lib
    at-spi2-core
  ]);
  pathsToLink = [ "/lib" ];
  ignoreCollisions = true;
}
