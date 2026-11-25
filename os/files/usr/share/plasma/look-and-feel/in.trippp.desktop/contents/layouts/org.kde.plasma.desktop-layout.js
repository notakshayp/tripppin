// The Tripppin desktop, first login (the "Tiles" design, mock section 02): flat #101418 on
// every screen (the desktop itself shows ~/Desktop as icons) and one thin taskbar docked at
// the bottom, 40 px, solid ink, square: the start button (a 40 x 40 accent square, runs
// `tripppin-ui start`), then icons-only running apps with a 2 px accent underline, then at the
// far right the status line in plain text ("Wi-Fi   82%   21:14") and the notifications bell.

var desktops = desktopsForActivity(currentActivity());
for (var i = 0; i < desktops.length; i++) {
    var d = desktops[i];
    d.wallpaperPlugin = "org.kde.color";
    d.currentConfigGroup = ["Wallpaper", "org.kde.color", "General"];
    d.writeConfig("Color", "#101418");
}

var panel = new Panel();
panel.location = "bottom";
panel.height = 40;
panel.floating = false;
panel.alignment = "left";
panel.lengthMode = "fill";
panel.hiding = "none";
panel.opacity = "opaque";

panel.addWidget("in.trippp.start");

var tasks = panel.addWidget("org.kde.plasma.icontasks");
tasks.currentConfigGroup = ["General"];
tasks.writeConfig("launchers", "applications:org.mozilla.firefox.desktop,applications:org.kde.dolphin.desktop,applications:org.kde.konsole.desktop,applications:systemsettings.desktop");
tasks.writeConfig("iconSpacing", "0");
tasks.writeConfig("maxStripes", "1");
tasks.writeConfig("indicateAudioStreams", "false");
tasks.writeConfig("showToolTips", "true");

panel.addWidget("org.kde.plasma.panelspacer");

panel.addWidget("in.trippp.status");

// The tray shows nothing but notifications (so they stay reachable); Wi-Fi, battery and the
// time are the status line's text.
var tray = panel.addWidget("org.kde.plasma.systemtray");
tray.currentConfigGroup = ["General"];
tray.writeConfig("extraItems", "org.kde.plasma.notifications");
tray.writeConfig("shownItems", "org.kde.plasma.notifications");
tray.writeConfig("hiddenItems", "");
tray.writeConfig("scaleIconsToFit", "true");
