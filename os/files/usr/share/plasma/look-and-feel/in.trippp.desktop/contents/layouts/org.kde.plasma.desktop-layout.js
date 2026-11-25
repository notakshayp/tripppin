// The Tripppin desktop, first login: the wallpaper on every screen (the desktop itself
// shows ~/Desktop as icons) and one floating panel: launcher, icons-only task manager,
// system tray, clock with the date.
var wallpaper = "file:///usr/share/tripppin/wallpapers/ink.png";

var desktops = desktopsForActivity(currentActivity());
for (var i = 0; i < desktops.length; i++) {
    var d = desktops[i];
    d.wallpaperPlugin = "org.kde.image";
    d.currentConfigGroup = ["Wallpaper", "org.kde.image", "General"];
    d.writeConfig("Image", wallpaper);
}

var panel = new Panel();
panel.location = "bottom";
panel.height = 2 * gridUnit + 8;
panel.floating = true;
panel.alignment = "center";
panel.lengthMode = "fit";
panel.hiding = "none";
panel.opacity = "adaptive";

var launcher = panel.addWidget("org.kde.plasma.kickoff");
launcher.currentConfigGroup = ["General"];
launcher.writeConfig("icon", "tripppin");

var tasks = panel.addWidget("org.kde.plasma.icontasks");
tasks.currentConfigGroup = ["General"];
tasks.writeConfig("launchers", "applications:org.mozilla.firefox.desktop,applications:org.kde.dolphin.desktop,applications:org.kde.konsole.desktop,applications:systemsettings.desktop");

panel.addWidget("org.kde.plasma.marginsseparator");
panel.addWidget("org.kde.plasma.systemtray");

var clock = panel.addWidget("org.kde.plasma.digitalclock");
clock.currentConfigGroup = ["Appearance"];
clock.writeConfig("showDate", "true");
clock.writeConfig("dateFormat", "shortDate");
clock.writeConfig("use24hFormat", "2");
