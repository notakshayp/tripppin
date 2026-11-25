// The Tripppin desktop, first login (the "Bento" design, mock section 02): #16181a on every
// screen (the desktop itself shows ~/Desktop as icons) and one 68 px rail down the left edge,
// full height, #1c1f21 (the Plasma style draws it): at the top the bridge tile (a 42 px amber
// rounded square with the dark logo, runs `tripppin-ui bridge`, and a short divider under it), pinned and running apps as
// 42 px icons (a running app has an amber bar at the rail's edge), then at the bottom the
// notifications bell and the stacked status (Wi-Fi, volume, battery %, time; a press opens
// `tripppin-ui quick`).

var desktops = desktopsForActivity(currentActivity());
for (var i = 0; i < desktops.length; i++) {
    var d = desktops[i];
    d.wallpaperPlugin = "org.kde.color";
    d.currentConfigGroup = ["Wallpaper", "org.kde.color", "General"];
    d.writeConfig("Color", "#16181a");
    // Plasma's own right-click menu is emptied: ours (tripppin-ui desktop, on the layer above
    // the wallpaper) has New folder, Change wallpaper and Display settings that open our
    // Settings, and Open terminal. Nothing here may lead to KDE's own configuration dialogs.
    d.currentConfigGroup = ["ActionPlugins", "0"];
    d.writeConfig("RightButton;NoModifier", "org.kde.contextmenu");
    d.currentConfigGroup = ["ActionPlugins", "0", "RightButton;NoModifier"];
    var hidden = ["_context", "_sep1", "_sep2", "_sep3", "_add widgets", "_add panel", "_manage_activities",
        "_wallpaper", "_lock_screen", "_logout", "_run_command", "_open_terminal", "_edit_mode", "_display_settings"];
    for (var h = 0; h < hidden.length; h++) {
        d.writeConfig(hidden[h], "false");
    }
}

var panel = new Panel();
panel.location = "left";
panel.height = 68;   // a vertical panel's thickness
panel.floating = false;
panel.alignment = "left";
panel.lengthMode = "fill";
panel.hiding = "none";
panel.opacity = "opaque";

panel.addWidget("in.trippp.bridge");

var tasks = panel.addWidget("org.kde.plasma.icontasks");
tasks.currentConfigGroup = ["General"];
tasks.writeConfig("launchers", "applications:org.mozilla.firefox.desktop,applications:org.kde.dolphin.desktop,applications:org.kde.konsole.desktop,applications:in.trippp.Settings.desktop");
tasks.writeConfig("iconSpacing", "1");
tasks.writeConfig("maxStripes", "1");
tasks.writeConfig("indicateAudioStreams", "false");
tasks.writeConfig("showToolTips", "true");

panel.addWidget("org.kde.plasma.panelspacer");

// The tray shows nothing but notifications (so they stay reachable: the bell).
var tray = panel.addWidget("org.kde.plasma.systemtray");
tray.currentConfigGroup = ["General"];
tray.writeConfig("extraItems", "org.kde.plasma.notifications");
tray.writeConfig("shownItems", "org.kde.plasma.notifications");
tray.writeConfig("hiddenItems", "");
tray.writeConfig("scaleIconsToFit", "true");

panel.addWidget("in.trippp.status");
