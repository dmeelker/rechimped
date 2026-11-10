Rechimped
=========

A small desktop app for viewing and editing your own TimeChimp time entries and travel,
without going through the web app.

Screenshots, the source and the full documentation: https://github.com/dmeelker/rechimped


Installing
----------

Windows: unzip the archive anywhere and run rechimped.exe. Nothing is installed.

Linux: run ./install.sh from the unpacked archive. It installs for the current user only:
the binary to ~/.local/bin, plus a menu entry and an icon under ~/.local/share.
./install.sh --uninstall removes them again. The app needs glibc 2.35 or newer, libxcb,
libxkbcommon (with its X11 part), OpenSSL 3 and a Vulkan driver; recent Fedora, Ubuntu
22.04 and Debian 12 have these.


Using it
--------

Log in with your normal TimeChimp account (MFA is supported). Everything can be done from
the keyboard; the footer lists the shortcuts. Ctrl+, opens the settings.

The app keeps a session token and a config.toml with your settings and presets in a
"rechimped" folder:

  Windows: %APPDATA%\rechimped\
  Linux:   $XDG_CONFIG_HOME/rechimped/ (usually ~/.config/rechimped/)

Delete that folder to remove everything it stored.


Disclaimer
----------

Rechimped is not affiliated with, endorsed by or in any way related to TimeChimp. It uses
TimeChimp's undocumented internal web API, the one their own web app uses, which can change
or break at any time.

The software is provided "as is", without warranty of any kind. It edits real data in your
TimeChimp account: use it at your own risk.


License
-------

Public domain, under the Unlicense: see UNLICENSE.
