# Smart Connect Linux Vibe 安装说明

**本项目修改自 [rudrahnpatel/smartconnect-linux](https://github.com/rudrahnpatel/smartconnect-linux)，保留原项目 MIT 许可证与版权声明。**

版本：**v0.3.0**。发布页：[smartconnect-linux-vibe Releases](https://github.com/2274268506/smartconnect-linux-vibe/releases)。

## Arch Linux / EndeavourOS 安装包

发布的 `.pkg.tar.zst` 面向 **x86_64、已完整更新的 Arch 系发行版**，使用系统 GTK、GStreamer 等动态库。它不是通用 Linux 安装包，不能直接安装到 Ubuntu 或 Fedora。本版没有发布 AppImage。

下载并校验：

```sh
mkdir -p smartconnect-vibe-install
cd smartconnect-vibe-install
curl -fLO https://github.com/2274268506/smartconnect-linux-vibe/releases/download/v0.3.0/smartconnect-linux-vibe-0.3.0-1-x86_64.pkg.tar.zst
curl -fLO https://github.com/2274268506/smartconnect-linux-vibe/releases/download/v0.3.0/SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
```

校验显示安装包 `OK` 后安装：

```sh
sudo pacman -Syu
sudo pacman -U ./smartconnect-linux-vibe-0.3.0-1-x86_64.pkg.tar.zst
# 可选功能：scrcpy 投屏、ADB、剪贴板同步
sudo pacman -S --needed scrcpy android-tools wl-clipboard
sudo systemctl enable --now avahi-daemon bluetooth
systemctl --user enable --now smartconnectd
smartconnect-gui
```

GUI 和 `systemctl --user` 都以普通桌面用户运行，**不要加 sudo**。蓝牙用于后台唤醒；不使用此功能时不必启用 bluetooth。

若此前通过源码或 AppImage 安装，先按旧安装方式卸载程序或停用旧启动器、用户服务覆盖，保留配对和设置。用 `command -v smartconnectd` 和 `systemctl --user cat smartconnectd` 检查是否仍指向旧版本；安装此包后通常应使用 `/usr/bin/smartconnectd`。此包与原 `smartconnect-linux` 包冲突，不能同时安装。

## 配对与投屏

1. 手机安装并打开官方 Smart Connect；电脑与手机在同一可互通的局域网。
2. 电脑左上角设备菜单选择 **配对新手机…**；手机选择 **+ → 扫描二维码**。
3. 按手机提示允许连接，并按需开启通知、短信等权限。
4. 使用 scrcpy 时，还需在手机开发者选项中启用并授权 USB/网络 ADB。网络 ADB 连接端口不一定是 5555，且不等于无线配对端口。
5. 应用页的投屏设置或桌面模式中选择 **scrcpy (ADB)**，设置分辨率、码率和帧率后开始。

手机密码锁仍需本人解锁。完整折叠鼠标兼容需要 Android 15+/scrcpy 4+；电脑输入法需要 scrcpy 5+。**独立于手机音量**需要 Android 13+，在当前设备已验证手机媒体音量为 0 时电脑仍有声音，但部分应用可能禁止音频采集。应用设置需保存并重开投屏后生效。

语言设置支持英文、简体中文、法文、西班牙文及跟随系统；全新配置默认英文，可在 Preferences 中切换语言并重启 GUI。

## 防火墙

需要允许手机访问控制端口 TCP 52020，以及 mDNS UDP 5353；文件共享服务需要 TCP 2049。仅对可信局域网开放。firewalld 示例：

```sh
sudo firewall-cmd --permanent --add-port=52020/tcp
sudo firewall-cmd --permanent --add-service=mdns
sudo firewall-cmd --reload
```

投屏媒体还使用 UDP，受防火墙、VPN、访客 Wi-Fi 隔离等影响。详细规则和 UFW 配置见 [安装文档](docs/installation.md#firewall)。

## 其他发行版源码安装

先按 [依赖清单](docs/installation.md#packages) 安装编译工具及开发库。当前 GUI 至少要求 **GTK 4.18、libadwaita 1.7**；视频功能使用 GStreamer、libnice 等。老版本发行版的默认软件源可能无法满足这些要求。

Arch 编译依赖示例：

```sh
sudo pacman -S --needed git base-devel rust pkgconf avahi bluez wl-clipboard \
  gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad libnice gst-libav \
  gst-plugin-va gtk4 libadwaita fuse3 sdl3 scrcpy android-tools
```

安装 SDL3 开发头文件后再构建，才能启用电脑输入法适配器。然后：

```sh
git clone https://github.com/2274268506/smartconnect-linux-vibe.git
cd smartconnect-linux-vibe
git checkout v0.3.0
./packaging/install.sh --enable
smartconnect-gui
```

也可从 Release 下载 `smartconnect-linux-vibe-0.3.0-source.tar.gz`，解压后运行同一安装脚本。源码方式默认安装到 `~/.local`，无需使用 sudo 运行安装脚本。

## 更新、卸载与排查

- 安装包更新：下载新版本后运行 `sudo pacman -U ./新安装包.pkg.tar.zst`，再运行 `systemctl --user restart smartconnectd` 并重开 GUI；正在进行的投屏会结束。
- 安装包卸载：先运行 `systemctl --user disable --now smartconnectd`，再运行 `sudo pacman -R smartconnect-linux-vibe`。
- 源码安装卸载：运行 `./packaging/uninstall.sh`，先用 `--help` 查看选项。个人配置和配对文件是否保留以脚本提示为准。
- 状态与日志：`smartconnectd status`、`journalctl --user -u smartconnectd -n 100 --no-pager`；投屏日志也可从 GUI 设备菜单查看。
- 本版限制：原生渲染在部分折叠设备强制旋转时可能比例异常，可使用 scrcpy；原生文件挂载仍以只读浏览为主；新增 ADB 文件上传/下载，但暂不支持递归传输文件夹。跨设备拖放和彩信附件等尚未完整实现。USB 实线连接和真实短信发送未在当前设备验证。

本版已验证 scrcpy 应用/桌面投屏在手机自动熄屏后继续工作。Motorola 双击亮屏补偿仍待实机确认；内置投屏仅能播放手机已协商的音轨，独立原生音频握手及双向音频尚未完成。
