import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.plasma5support as P5Support
import org.kde.plasma.private.battery

// The taskbar's right end (mock section 02): plain 12 px text, "Wi-Fi   82%   21:14". Wi-Fi
// shows while connected over Wi-Fi ("Wired" over a cable), the percentage only on a
// computer with a battery. Nothing is invented: a part with nothing to say is left out.
PlasmoidItem {
    id: root
    preferredRepresentation: fullRepresentation

    property string net: ""
    property date now: new Date()

    BatteryControlModel { id: battery }

    Timer {
        interval: 1000
        running: true
        repeat: true
        onTriggered: root.now = new Date()
    }

    P5Support.DataSource {
        id: nm
        engine: "executable"
        connectedSources: ["nmcli -t -f TYPE,STATE device"]
        interval: 10000
        onNewData: (source, data) => {
            var out = data["stdout"] || "";
            if (out.indexOf("wifi:connected") >= 0 && out.indexOf("wifi:connected (") < 0)
                root.net = "Wi-Fi";
            else if (out.indexOf("ethernet:connected") >= 0)
                root.net = "Wired";
            else
                root.net = "";
        }
    }

    fullRepresentation: Item {
        Layout.minimumWidth: label.implicitWidth + 24
        Layout.preferredWidth: label.implicitWidth + 24
        Layout.minimumHeight: 40

        Text {
            id: label
            anchors.centerIn: parent
            color: "#ffffff"
            font.family: "Inter"
            font.pixelSize: 12
            textFormat: Text.PlainText
            text: {
                var parts = [];
                if (root.net !== "") parts.push(root.net);
                if (battery.hasInternalBatteries) parts.push(battery.percent + "%");
                parts.push(Qt.formatTime(root.now, "HH:mm"));
                return parts.join("   ");
            }
        }
    }
}
