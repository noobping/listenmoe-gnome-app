
# A tiny Gnome App for LISTEN.moe

Anime/Japanese Radio powered by LISTEN.moe!

![screenshot](data/screenshot.png)

## Flatpak App

Install the app from my Flatpak Repo:

```sh
flatpak remote-add --if-not-exists flatpaks https://noobping.github.io/flatpaks/flatpaks.flatpakrepo ;\
flatpak install flatpaks dev.noobping.listenmoe
```

## Stand-alone Executable

You can download a stand-alone binary from the [GitHub Releases](https://github.com/noobping/listenmoe-gnome-app/releases/latest) page.
After downloading:

1. Mark it as executable:

```sh
chmod +x ./listenmoe.linux.x86_64 
```

2. Run the executable.

While the app is running, press `F2` to install or uninstall it locally. The app will place (or remove) its files in the user data directory: `~/.local`.
